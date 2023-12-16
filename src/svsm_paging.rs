// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2022-2023 SUSE LLC
//
// Author: Joerg Roedel <jroedel@suse.de>

use crate::address::{Address, PhysAddr, VirtAddr};
use crate::config::SvsmConfig;
use crate::cpu::percpu::this_cpu_mut;
use crate::elf;
use crate::error::SvsmError;
use crate::igvm_params::IgvmParams;
use crate::mm;
use crate::mm::pagetable::{set_init_pgtable, PTEntryFlags, PageTable, PageTableRef};
use crate::mm::PerCPUPageMappingGuard;
use crate::sev::ghcb::PageStateChangeOp;
use crate::sev::{pvalidate, PvalidateOp};
use crate::types::{PageSize, PAGE_SIZE};
use bootlib::kernel_launch::KernelLaunchInfo;

struct IgvmParamInfo<'a> {
    virt_addr: VirtAddr,
    igvm_params: Option<IgvmParams<'a>>,
}

pub fn init_page_table(launch_info: &KernelLaunchInfo, kernel_elf: &elf::Elf64File) {
    let vaddr = mm::alloc::allocate_zeroed_page().expect("Failed to allocate root page-table");
    let mut pgtable = PageTableRef::new(unsafe { &mut *vaddr.as_mut_ptr::<PageTable>() });
    let igvm_param_info = if launch_info.igvm_params_virt_addr != 0 {
        let addr = VirtAddr::from(launch_info.igvm_params_virt_addr);
        IgvmParamInfo {
            virt_addr: addr,
            igvm_params: Some(IgvmParams::new(addr)),
        }
    } else {
        IgvmParamInfo {
            virt_addr: VirtAddr::null(),
            igvm_params: None,
        }
    };

    // Install mappings for the kernel's ELF segments each.
    // The memory backing the kernel ELF segments gets allocated back to back
    // from the physical memory region by the Stage2 loader.
    let mut phys = PhysAddr::from(launch_info.kernel_region_phys_start);
    if let Some(ref igvm_params) = igvm_param_info.igvm_params {
        phys = phys + igvm_params.reserved_kernel_area_size();
    }

    for segment in kernel_elf.image_load_segment_iter(launch_info.kernel_region_virt_start) {
        let vaddr_start = VirtAddr::from(segment.vaddr_range.vaddr_begin);
        let vaddr_end = VirtAddr::from(segment.vaddr_range.vaddr_end);
        let aligned_vaddr_end = vaddr_end.page_align_up();
        let segment_len = aligned_vaddr_end - vaddr_start;
        let flags = if segment.flags.contains(elf::Elf64PhdrFlags::EXECUTE) {
            PTEntryFlags::exec()
        } else if segment.flags.contains(elf::Elf64PhdrFlags::WRITE) {
            PTEntryFlags::data()
        } else {
            PTEntryFlags::data_ro()
        };

        pgtable
            .map_region(vaddr_start, aligned_vaddr_end, phys, flags)
            .expect("Failed to map kernel ELF segment");

        phys = phys + segment_len;
    }

    // Map the IGVM parameters if present.
    if let Some(ref igvm_params) = igvm_param_info.igvm_params {
        pgtable
            .map_region(
                igvm_param_info.virt_addr,
                igvm_param_info.virt_addr + igvm_params.size(),
                PhysAddr::from(launch_info.igvm_params_phys_addr),
                PTEntryFlags::data(),
            )
            .expect("Failed to map IGVM parameters");
    }

    // Map subsequent heap area.
    pgtable
        .map_region(
            VirtAddr::from(launch_info.heap_area_virt_start),
            VirtAddr::from(launch_info.heap_area_virt_end()),
            PhysAddr::from(launch_info.heap_area_phys_start),
            PTEntryFlags::data(),
        )
        .expect("Failed to map heap");

    pgtable.load();

    set_init_pgtable(pgtable);
}

pub fn invalidate_stage2(config: &SvsmConfig) -> Result<(), SvsmError> {
    let pstart = PhysAddr::null();
    let pend = pstart + (640 * 1024);
    let mut paddr = pstart;

    // Stage2 memory must be invalidated when already on the SVSM page-table,
    // because before that the stage2 page-table is still active, which is in
    // stage2 memory, causing invalidation of page-table pages.
    while paddr < pend {
        let guard = PerCPUPageMappingGuard::create_4k(paddr)?;
        let vaddr = guard.virt_addr();

        pvalidate(vaddr, PageSize::Regular, PvalidateOp::Invalid)?;

        paddr = paddr + PAGE_SIZE;
    }

    if config.page_state_change_required() {
        this_cpu_mut()
            .ghcb()
            .page_state_change(paddr, pend, PageSize::Regular, PageStateChangeOp::PscShared)
            .expect("Failed to invalidate Stage2 memory");
    }

    Ok(())
}
