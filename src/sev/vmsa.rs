// SPDX-License-Identifier: (GPL-2.0-or-later OR MIT)
//
// Copyright (c) 2022 SUSE LLC
//
// Author: Joerg Roedel <jroedel@suse.de>
//
// vim: ts=4 sw=4 et

use crate::mm::alloc::{allocate_zeroed_page, free_page};
use super::utils::{RMPFlags, rmp_adjust};

pub const VMPL_MAX  : usize = 4;

#[repr(C, packed)]
struct VMSASegment {
    selector    : u16,
    flags       : u16,
    limit       : u32,
    base        : u64,
}

#[repr(C, packed)]
pub struct VMSA {
    es                  : VMSASegment,
    cs                  : VMSASegment,
    ss                  : VMSASegment,
    ds                  : VMSASegment,
    fs                  : VMSASegment,
    gs                  : VMSASegment,
    gdt                 : VMSASegment,
    ldt                 : VMSASegment,
    idt                 : VMSASegment,
    tr                  : VMSASegment,
    pl0_ssp             : u64,
    pl1_ssp             : u64,
    pl2_ssp             : u64,
    pl3_ssp             : u64,
    u_cet               : u64,
    reserved_0c8        : u16,
    vmpl                : u8,
    cpl                 : u8,
    reserved_0cc        : u32,
    efer                : u64,
    reserved_0d8        : [u8; 104],
    xss                 : u64,
    cr4                 : u64,
    cr3                 : u64,
    cr0                 : u64,
    dr7                 : u64,
    dr6                 : u64,
    rflags              : u64,
    rip                 : u64,
    dr0                 : u64,
    dr1                 : u64,
    dr2                 : u64,
    dr3                 : u64,
    dr0_mask            : u64,
    dr1_mask            : u64,
    dr2_mask            : u64,
    dr3_mask            : u64,
    reserved_1c0        : [u8; 24],
    rsp                 : u64,
    s_cet               : u64,
    ssp                 : u64,
    isst_addr           : u64,
    rax                 : u64,
    star                : u64,
    lstar               : u64,
    cstar               : u64,
    sfmask              : u64,
    kernel_gs_base      : u64,
    sysenter_cs         : u64,
    sysenter_esp        : u64,
    sysenter_eip        : u64,
    cr2                 : u64,
    reserved_248        : [u8; 32],
    g_pat               : u64,
    dbgctl              : u64,
    br_from             : u64,
    br_to               : u64,
    last_excp_from      : u64,
    last_excp_to        : u64,
    reserved_298        : [u8; 72],
    reserved_2e0        : u64,
    pkru                : u32,
    reserved_2ec        : u32,
    guest_tsc_scale     : u64,
    guest_tsc_offset    : u64,
    reg_prot_nonce      : u64,
    rcx                 : u64,
    rdx                 : u64,
    rbx                 : u64,
    reserved_320        : u64,
    rbp                 : u64,
    rsi                 : u64,
    rdi                 : u64,
    r8                  : u64,
    r9                  : u64,
    r10                 : u64,
    r11                 : u64,
    r12                 : u64,
    r13                 : u64,
    r14                 : u64,
    r15                 : u64,
    reserved_380        : [u8; 16],
    guest_exitinfo1     : u64,
    guest_exitinfo2     : u64,
    guest_exitintinfo   : u64,
    guest_nrip          : u64,
    sev_features        : u64,
    vintr_ctrl          : u64,
    guest_exit_code     : u64,
    vtom                : u64,
    tlb_id              : u64,
    pcpu_id             : u64,
    event_inj           : u64,
    xcr0                : u64,
    reserved_3f0        : [u8; 16],
    x87_dp              : u64,
    mxcsr               : u32,
    x87_ftw             : u16,
    x87_fsw             : u16,
    x87_fcw             : u16,
    x87_fop             : u16,
    x87_ds              : u16,
    x87_cs              : u16,
    x87_rip             : u64,
    fpreg_x87           : [u8; 80],
    fpreg_xmm           : [u8; 256],
    fpreg_ymm           : [u8; 256],
    reserved_670        : [u8; 2448],
}

pub fn allocate_new_vmsa() -> Result<*mut VMSA, ()> {
    let vmsa_page = allocate_zeroed_page()?;
    if let Err(_e) = rmp_adjust(vmsa_page, RMPFlags::VMPL1_VMSA, false) {
        free_page(vmsa_page);
        return Err(())
    }
    Ok(vmsa_page as *mut VMSA)
}
