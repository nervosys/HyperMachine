//! Architecture-specific code for Type-1 hypervisor
//!
//! This module contains x86_64-specific implementations including:
//! - GDT (Global Descriptor Table) setup
//! - IDT (Interrupt Descriptor Table) setup
//! - CPU feature detection
//! - Control register management

use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::idt::InterruptDescriptorTable;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

/// Size of the interrupt stack
pub const INTERRUPT_STACK_SIZE: usize = 4096 * 5;

/// Double fault stack index in TSS
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

/// GDT selectors
#[derive(Debug, Clone, Copy)]
pub struct Selectors {
    /// Code segment selector
    pub code_selector: SegmentSelector,
    /// Data segment selector  
    pub data_selector: SegmentSelector,
    /// TSS selector
    pub tss_selector: SegmentSelector,
}

/// Initialize the GDT with TSS
pub fn init_gdt(tss: &'static TaskStateSegment) -> (GlobalDescriptorTable, Selectors) {
    let mut gdt = GlobalDescriptorTable::new();
    
    let code_selector = gdt.append(Descriptor::kernel_code_segment());
    let data_selector = gdt.append(Descriptor::kernel_data_segment());
    let tss_selector = gdt.append(Descriptor::tss_segment(tss));
    
    let selectors = Selectors {
        code_selector,
        data_selector,
        tss_selector,
    };
    
    (gdt, selectors)
}

/// Create a new TSS with interrupt stacks
pub fn create_tss(stack: &'static [u8; INTERRUPT_STACK_SIZE]) -> TaskStateSegment {
    let mut tss = TaskStateSegment::new();
    
    // Set up the double fault stack
    tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
        let stack_start = VirtAddr::from_ptr(stack.as_ptr());
        stack_start + INTERRUPT_STACK_SIZE as u64
    };
    
    tss
}

/// CPU feature flags
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuFeatures {
    /// SSE support
    pub sse: bool,
    /// SSE2 support
    pub sse2: bool,
    /// SSE3 support
    pub sse3: bool,
    /// SSSE3 support
    pub ssse3: bool,
    /// SSE4.1 support
    pub sse4_1: bool,
    /// SSE4.2 support
    pub sse4_2: bool,
    /// AVX support
    pub avx: bool,
    /// AVX2 support
    pub avx2: bool,
    /// XSAVE support
    pub xsave: bool,
    /// FXSAVE/FXRSTOR support
    pub fxsr: bool,
    /// VMX support (Intel)
    pub vmx: bool,
    /// SVM support (AMD)
    pub svm: bool,
    /// 1GB pages support
    pub page_1gb: bool,
    /// NX bit support
    pub nx: bool,
    /// RDTSCP support
    pub rdtscp: bool,
    /// INVPCID support
    pub invpcid: bool,
}

impl CpuFeatures {
    /// Detect CPU features
    pub fn detect() -> Self {
        let cpuid = raw_cpuid::CpuId::new();
        
        let mut features = CpuFeatures::default();
        
        if let Some(info) = cpuid.get_feature_info() {
            features.sse = info.has_sse();
            features.sse2 = info.has_sse2();
            features.sse3 = info.has_sse3();
            features.ssse3 = info.has_ssse3();
            features.sse4_1 = info.has_sse41();
            features.sse4_2 = info.has_sse42();
            features.fxsr = info.has_fxsave_fxstor();
            features.xsave = info.has_xsave();
            features.vmx = info.has_vmx();
        }
        
        if let Some(info) = cpuid.get_extended_feature_info() {
            features.avx2 = info.has_avx2();
            features.invpcid = info.has_invpcid();
        }
        
        if let Some(info) = cpuid.get_extended_processor_and_feature_identifiers() {
            features.svm = info.has_svm();
            features.page_1gb = info.has_1gib_pages();
            features.nx = info.has_execute_disable();
            features.rdtscp = info.has_rdtscp();
        }
        
        features
    }
}

/// Read the current instruction pointer
#[inline]
pub fn read_rip() -> u64 {
    let rip: u64;
    unsafe {
        core::arch::asm!(
            "lea {}, [rip]",
            out(reg) rip,
            options(nostack, nomem)
        );
    }
    rip
}

/// Read the current stack pointer
#[inline]
pub fn read_rsp() -> u64 {
    let rsp: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, rsp",
            out(reg) rsp,
            options(nostack, nomem)
        );
    }
    rsp
}

/// Read RFLAGS register
#[inline]
pub fn read_rflags() -> u64 {
    let rflags: u64;
    unsafe {
        core::arch::asm!(
            "pushfq",
            "pop {}",
            out(reg) rflags,
            options(nomem)
        );
    }
    rflags
}

/// Halt the CPU until the next interrupt
#[inline]
pub fn hlt() {
    unsafe {
        core::arch::asm!("hlt", options(nomem, nostack));
    }
}

/// Disable interrupts
#[inline]
pub fn cli() {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
}

/// Enable interrupts
#[inline]
pub fn sti() {
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

/// Pause instruction for spin loops
#[inline]
pub fn pause() {
    unsafe {
        core::arch::asm!("pause", options(nomem, nostack));
    }
}
