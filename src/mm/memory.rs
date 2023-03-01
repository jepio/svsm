// SPDX-License-Identifier: (GPL-2.0-or-later OR MIT)
//
// Copyright (c) 2022 SUSE LLC
//
// Author: Joerg Roedel <jroedel@suse.de>
//
// vim: ts=4 sw=4 et

extern crate alloc;

use crate::types::PhysAddr;
use crate::kernel_launch::KernelLaunchInfo;
use crate::fw_cfg::{FwCfg, MemoryRegion};
use alloc::vec::Vec;
use log;

static mut MEMORY_MAP: Vec<MemoryRegion> = Vec::new();


pub fn init_memory_map(fwcfg: &FwCfg, launch_info: &KernelLaunchInfo) -> Result<(),()> {
    let mut regions = fwcfg.get_memory_regions()?;

    // Remove SVSM memory from guest memory map
    for mut region in regions.iter_mut() {
        if (launch_info.kernel_start > region.start) &&
           (launch_info.kernel_start < region.end) {
               region.end = launch_info.kernel_start;
           }
    }

    log::info!("Guest Memory Regions:");
    for r in regions.iter() {
        log::info!("  {:018x}-{:018x}", r.start, r.end);
    }

    unsafe { MEMORY_MAP = regions; }

    Ok(())
}

pub fn valid_phys_address(addr: PhysAddr) -> bool {
    let addr = addr as u64;
    unsafe {
        MEMORY_MAP.iter()
            .any(|region| addr >= region.start && addr < region.end )
    }
}
