# NeoDOS Glossary

| Term | Definition |
|------|------------|
| ABI | Application Binary Interface — the stable contract between NEM drivers and the kernel |
| ACL | Access Control List — list of ACEs attached to an ObObject for security |
| Cm | Configuration Manager — the registry subsystem |
| DPC | Deferred Procedure Call — interrupt work deferred to DISPATCH_LEVEL |
| ECAM | Enhanced Configuration Access Mechanism — MMIO-based PCI config space access |
| GPT | GUID Partition Table — disk partition layout used by NeoDOS |
| HAL | Hardware Abstraction Layer — platform abstraction for x86_64 |
| IDT | Interrupt Descriptor Table — maps 256 interrupt vectors to handlers |
| IOAPIC | I/O Advanced Programmable Interrupt Controller — routes device IRQs |
| IPC | Inter-Process Communication — pipes, IRP, event bus |
| IPI | Inter-Processor Interrupt — SMP cross-CPU signalling |
| IRP | I/O Request Packet — async I/O operation descriptor |
| IRQL | Interrupt Request Level — per-CPU interrupt priority (0/PASSIVE to 15/HIGH) |
| KCR | Kernel Certification Requirements — compliance rules for NEM drivers |
| KPRCB | Kernel Processor Control Block — per-CPU structure (GS segment) |
| MSI-X | Message Signalled Interrupts eXtended — PCIe MSI-X capability |
| MCFG | Memory-mapped Configuration space — ACPI table for ECAM base address |
| NEM | NeoDOS External Module — driver format with certification pipeline |
| NeoFS | NeoDOS File System — custom journaling filesystem (v2 = NE2) |
| NLT | NeoDOS Language Technology — i18n format and toolchain |
| NXE | NeoDOS eXecutable — user-mode binary format (ELF + NXE metadata) |
| NXL | NeoDOS Library — dynamic library (.NXL) loaded by NXE binaries |
| NXP | NeoDOS eXecutable Package — package container format |
| Ob | Object Manager — central kernel object graph unifying handles, namespace, security |
| ObId | Unique 64-bit object identifier, monotonically increasing |
| ObType | Object type discriminator (Process, Pipe, Event, etc.) |
| OVMF | Open Virtual Machine Firmware — UEFI firmware for QEMU |
| SID | Security Identifier — uniquely identifies a user or group |
| SMP | Symmetric Multi-Processing — multiple CPU cores |
| SSDT | System Service Dispatch Table — syscall dispatch (256 slots, RAX indexed) |
| TSS | Task State Segment — provides IST stacks for exceptions |
| UEFI | Unified Extensible Firmware Interface — modern BIOS replacement |
| URN | Unified Resource Name — URI scheme resolving via Ob namespace |
| VFS | Virtual File System — abstraction layer over filesystem implementations |
| VT | Virtual Terminal — console session (Alt+F1-F4) |
