//! A 6502 NES Central Processing Unit
//!
//! http://wiki.nesdev.com/w/index.php/CPU

use crate::console::cartridge::{Board, BoardRef};
use crate::console::memory::{Addr, Byte, CpuMemMap, Memory, Word};
use std::fmt;

pub type Cycles = u64;

// 1.79 MHz (~559 ns/cycle) - May want to use 1_786_830 for a stable 60 FPS
// const CPU_CLOCK_FREQ: Frequency = 1_789_773.0;
const NMI_ADDR: Addr = 0xFFFA;
const IRQ_ADDR: Addr = 0xFFFE;
const RESET_ADDR: Addr = 0xFFFC;
const POWER_ON_SP: Byte = 0xFD; // FD because reasons. Possibly because of NMI/IRQ/BRK messing with SP on reset
const POWER_ON_STATUS: Byte = 0x24; // 0010 0100 - Unused and Interrupt Disable set

// Status Registers
// http://wiki.nesdev.com/w/index.php/Status_flags
// 7654 3210
// NVUB DIZC
// |||| ||||
// |||| |||+- Carry
// |||| ||+-- Zero
// |||| |+--- Interrupt Disable
// |||| +---- Decimal - Not used in the NES but still has to function
// |||+------ Break Flag - 1 when pushed to stack from PHP/BRK, 0 from IRQ/NMI
// ||+------- Unused - always set to 1 when pushed to stack
// |+-------- Overflow
// +--------- Negative
const CARRY_FLAG: Byte = 1;
const ZERO_FLAG: Byte = 1 << 1;
const INTERRUPTD_FLAG: Byte = 1 << 2;
const DECIMAL_FLAG: Byte = 1 << 3;
const BREAK_FLAG: Byte = 1 << 4;
const UNUSED_FLAG: Byte = 1 << 5;
const OVERFLOW_FLAG: Byte = 1 << 6;
const NEGATIVE_FLAG: Byte = 1 << 7;

const CPU_TRACE_LOG: &str = "logs/cpu.log";

/// The Central Processing Unit
pub struct Cpu {
    pub mem: CpuMemMap,
    pub oplog: String,
    cycles: Cycles, // number of cycles
    stall: Cycles,  // number of cycles to stall/nop used mostly by write_oamdma
    pc: Addr,       // program counter
    sp: Byte,       // stack pointer - stack is at $0100-$01FF
    acc: Byte,      // accumulator
    x: Byte,        // x register
    y: Byte,        // y register
    status: Byte,
    trace: bool,
}

impl Cpu {
    pub fn init(mut mem: CpuMemMap) -> Self {
        let pc = mem.readw(RESET_ADDR);
        Self {
            cycles: 7,
            stall: 0,
            pc,
            sp: POWER_ON_SP,
            acc: 0,
            x: 0,
            y: 0,
            status: POWER_ON_STATUS,
            mem,
            trace: false,
            oplog: String::with_capacity(9000 * 60),
        }
    }

    /// Power cycles the CPU
    ///
    /// Updates all status as if powered on for the first time
    ///
    /// These operations take the CPU 7 cycles.
    pub fn power_cycle(&mut self) {
        self.cycles = 7;
        self.stall = 0;
        self.pc = self.mem.readw(RESET_ADDR);
        self.sp = POWER_ON_SP;
        self.acc = 0;
        self.x = 0;
        self.y = 0;
        self.status = POWER_ON_STATUS;
        self.oplog.clear();
    }

    /// Resets the CPU
    ///
    /// Updates the PC, SP, and Status values to defined constants.
    ///
    /// These operations take the CPU 7 cycles.
    pub fn reset(&mut self) {
        self.pc = self.mem.readw(RESET_ADDR);
        self.sp = self.sp.saturating_sub(3);
        self.set_interrupt_disable(true);
        self.cycles = 7;
    }

    /// Runs the CPU the passed in number of cycles
    pub fn step(&mut self) -> Cycles {
        if self.stall > 0 {
            self.stall -= 1;
        }
        let start_cycles = self.cycles;
        let opcode = self.readb(self.pc);
        self.execute(opcode);
        self.cycles - start_cycles
    }

    /// Sends an IRQ Interrupt to the CPU
    ///
    /// http://wiki.nesdev.com/w/index.php/IRQ
    pub fn irq(&mut self) {
        if self.interrupt_disable() {
            return;
        }
        self.push_stackw(self.pc);
        self.push_stackb((self.status | UNUSED_FLAG) & !BREAK_FLAG);
        self.status |= INTERRUPTD_FLAG;
        self.pc = self.mem.readw(IRQ_ADDR);
        self.cycles = self.cycles.wrapping_add(7);
        self.set_interrupt_disable(true);
    }

    /// Sends a NMI Interrupt to the CPU
    ///
    /// http://wiki.nesdev.com/w/index.php/NMI
    pub fn nmi(&mut self) {
        self.push_stackw(self.pc);
        self.push_stackb((self.status | UNUSED_FLAG) & !BREAK_FLAG);
        self.pc = self.mem.readw(NMI_ADDR);
        self.cycles = self.cycles.wrapping_add(7);
        self.set_interrupt_disable(true);
    }

    // Executes a single CPU instruction
    fn execute(&mut self, opcode: Byte) {
        let (op, addr_mode, cycles, page_cycles) = OPCODES[opcode as usize];
        let (val, target, num_args, page_crossed) =
            self.decode_addr_mode(addr_mode, self.pc.wrapping_add(1), op);
        if self.trace {
            self.print_instruction(op, opcode, 1 + num_args);
        }
        self.pc = self.pc.wrapping_add(1 + Word::from(num_args));
        self.cycles += cycles;
        if page_crossed {
            self.cycles += page_cycles;
        }

        let val = val as Byte;
        // Ordered by most often executed (roughly) to improve linear search time
        match op {
            LDA => self.lda(val),             // LoaD A with M
            BNE => self.bne(val),             // Branch if Not Equal to zero
            JMP => self.jmp(target.unwrap()), // JuMP
            INX => self.inx(),                // INcrement X
            BPL => self.bpl(val),             // Branch on PLus (positive)
            CMP => self.cmp(val),             // CoMPare
            BMI => self.bmi(val),             // Branch on MInus (negative)
            BEQ => self.beq(val),             // Branch if EQual to zero
            BIT => self.bit(val),             // Test BITs of M with A (Affects N, V and Z)
            STA => self.sta(target),          // STore A into M
            DEX => self.dex(),                // DEcrement X
            INY => self.iny(),                // INcrement Y
            TAY => self.tay(),                // Transfer A to Y
            INC => self.inc(target),          // INCrement M or A
            BCS => self.bcs(val),             // Branch if Carry Set
            JSR => self.jsr(target.unwrap()), // Jump and Save Return addr
            LSR => self.lsr(target),          // Logical Shift Right M or A
            RTS => self.rts(),                // ReTurn from Subroutine
            AND => self.and(val),             // AND M with A
            CLC => self.clc(),                // CLear Carry flag
            NOP => self.nop(),                // NO oPeration
            BCC => self.bcc(val),             // Branch on Carry Clear
            BVS => self.bvs(val),             // Branch on oVerflow Set
            SEC => self.sec(),                // SEt Carry flag
            BVC => self.bvc(val),             // Branch if no oVerflow Set
            LDY => self.ldy(val),             // LoaD Y with M
            CLV => self.clv(),                // CLear oVerflow flag
            LDX => self.ldx(val),             // LoaD X with M
            PLA => self.pla(),                // PulL A from the stack
            CPX => self.cpx(val),             // ComPare with X
            PHA => self.pha(),                // PusH A to the stack
            CPY => self.cpy(val),             // ComPare with Y
            PHP => self.php(),                // PusH Processor status to the stack
            SBC => self.sbc(val),             // Subtract M from A with carry
            PLP => self.plp(),                // PulL Processor status from the stack
            ADC => self.adc(val),             // ADd with Carry M with A
            DEC => self.dec(target),          // DECrement M or A
            ORA => self.ora(val),             // OR with A
            EOR => self.eor(val),             // Exclusive-OR M with A
            ROR => self.ror(target),          // ROtate Right M or A
            ROL => self.rol(target),          // ROtate Left M or A
            ASL => self.asl(target),          // Arithmatic Shift Left M or A
            STX => self.stx(target),          // STore X into M
            TAX => self.tax(),                // Transfer A to X
            TSX => self.tsx(),                // Transfer SP to X
            STY => self.sty(target),          // STore Y into M
            TXS => self.txs(),                // Transfer X to SP
            DEY => self.dey(),                // DEcrement Y
            TYA => self.tya(),                // Transfer Y to A
            TXA => self.txa(),                // TRansfer X to A
            SED => self.sed(),                // SEt Decimal mode
            RTI => self.rti(),                // ReTurn from Interrupt
            CLD => self.cld(),                // CLear Decimal mode
            SEI => self.sei(),                // SEt Interrupt disable
            CLI => self.cli(),                // CLear Interrupt disable
            BRK => self.brk(),                // BReaK (forced interrupt)
            KIL => self.kil(),                // KILl (stops CPU)
            ISB => self.isb(target),          // INC & SBC
            DCP => self.dcp(target),          // DEC & CMP
            AXS => self.axs(),                // A & X into X
            LAS => self.las(val),             // LDA & TSX
            LAX => self.lax(val),             // LDA & TAX
            AHX => self.ahx(),                // Store A & X & H in M
            SAX => self.sax(target),          // Sotre A & X in M
            XAA => self.xaa(),                // TXA & AND
            SHX => self.shx(),                // Store X & H in M
            RRA => self.rra(target),          // ROR & ADC
            TAS => self.tas(target),          // STA & TXS
            SHY => self.shy(),                // Store Y & H in M
            ARR => self.arr(),                // AND & ROR
            SRE => self.sre(target),          // LSR & EOR
            ALR => self.alr(),                // AND & LSR
            RLA => self.rla(target),          // ROL & AND
            ANC => self.anc(),                // AND & ASL
            SLO => self.slo(target),          // ASL & ORA
            _ => eprintln!("unhandled operation {:?}", op),
        };
    }

    // Getters/Setters

    // Updates the accumulator register
    fn update_acc(&mut self) {
        self.set_result_flags(self.acc);
    }

    // Sets the zero and negative registers appropriately
    fn set_result_flags(&mut self, val: Byte) {
        match val {
            0 => {
                self.set_zero(true);
                self.set_negative(false);
            }
            v if is_negative(v) => {
                self.set_zero(false);
                self.set_negative(true);
            }
            _ => {
                self.set_zero(false);
                self.set_negative(false);
            }
        }
    }

    // Used for testing to manually set the PC to a known value
    pub fn set_pc(&mut self, addr: Addr) {
        self.pc = addr;
    }

    // Used for testing to print a log of CPU instructions
    pub fn set_trace(&mut self, val: bool) {
        use std::fs;
        self.trace = val;
        fs::write(CPU_TRACE_LOG, "").expect("cpu trace log");
    }

    // Status Register functions

    fn carry(&self) -> bool {
        (self.status & CARRY_FLAG) == CARRY_FLAG
    }
    fn set_carry(&mut self, val: bool) {
        if val {
            self.status |= CARRY_FLAG;
        } else {
            self.status &= !CARRY_FLAG;
        }
    }

    fn zero(&self) -> bool {
        (self.status & ZERO_FLAG) == ZERO_FLAG
    }
    fn set_zero(&mut self, val: bool) {
        if val {
            self.status |= ZERO_FLAG;
        } else {
            self.status &= !ZERO_FLAG;
        }
    }

    fn interrupt_disable(&self) -> bool {
        (self.status & INTERRUPTD_FLAG) == INTERRUPTD_FLAG
    }
    fn set_interrupt_disable(&mut self, val: bool) {
        if val {
            self.status |= INTERRUPTD_FLAG;
        } else {
            self.status &= !INTERRUPTD_FLAG;
        }
    }

    fn set_decimal(&mut self, val: bool) {
        if val {
            self.status |= DECIMAL_FLAG;
        } else {
            self.status &= !DECIMAL_FLAG;
        }
    }

    fn overflow(&self) -> bool {
        (self.status & OVERFLOW_FLAG) == OVERFLOW_FLAG
    }
    fn set_overflow(&mut self, val: bool) {
        if val {
            self.status |= OVERFLOW_FLAG;
        } else {
            self.status &= !OVERFLOW_FLAG;
        }
    }

    fn negative(&self) -> bool {
        (self.status & NEGATIVE_FLAG) == NEGATIVE_FLAG
    }
    fn set_negative(&mut self, val: bool) {
        if val {
            self.status |= NEGATIVE_FLAG;
        } else {
            self.status &= !NEGATIVE_FLAG;
        }
    }

    // Stack Functions

    // Push a byte to the stack
    fn push_stackb(&mut self, val: Byte) {
        self.writeb(0x100 | Addr::from(self.sp), val);
        self.sp = self.sp.wrapping_sub(1);
    }

    // Pull a byte from the stack
    fn pop_stackb(&mut self) -> Byte {
        self.sp = self.sp.wrapping_add(1);
        self.readb(0x100 | Addr::from(self.sp))
    }

    // Push a word (two bytes) to the stack
    fn push_stackw(&mut self, val: u16) {
        let lo = (val & 0xFF) as Byte;
        let hi = (val >> 8) as Byte;
        self.push_stackb(hi);
        self.push_stackb(lo);
    }

    // Pull a word (two bytes) from the stack
    fn pop_stackw(&mut self) -> Word {
        let lo = Word::from(self.pop_stackb());
        let hi = Word::from(self.pop_stackb());
        hi << 8 | lo
    }

    // Decodes the addressing mode of the instruction and returns the target value, address (if
    // there is one), number of bytes used after the opcode, and whether it crossed a page
    // boundary
    fn decode_addr_mode(
        &mut self,
        mode: AddrMode,
        addr: Addr,
        op: Operation,
    ) -> (Word, Option<Addr>, u8, bool) {
        // Whether to read from memory or not
        // ST* opcodes only require the address not the value
        // so this saves unnecessary memory reads
        let read = match op {
            STA | STX | STY => false,
            _ => true,
        };
        match mode {
            Immediate => {
                let val = if read {
                    Addr::from(self.readb(addr))
                } else {
                    0
                };
                (val, Some(addr), 1, false)
            }
            ZeroPage => {
                let addr = Addr::from(self.readb(addr));
                let val = if read {
                    Word::from(self.readb(addr))
                } else {
                    0
                };
                (val, Some(addr), 1, false)
            }
            ZeroPageX => {
                let addr = Addr::from(self.readb(addr).wrapping_add(self.x));
                let val = if read {
                    Word::from(self.readb(addr))
                } else {
                    0
                };
                (val, Some(addr), 1, false)
            }
            ZeroPageY => {
                let addr = Addr::from(self.readb(addr).wrapping_add(self.y));
                let val = if read {
                    Word::from(self.readb(addr))
                } else {
                    0
                };
                (val, Some(addr), 1, false)
            }
            Absolute => {
                let addr = self.mem.readw(addr);
                let val = if read {
                    Word::from(self.readb(addr))
                } else {
                    0
                };
                (val, Some(addr), 2, false)
            }
            AbsoluteX => {
                let addr0 = self.mem.readw(addr);
                let addr = addr0.wrapping_add(Addr::from(self.x));
                // dummy read
                if ((addr0 & 0xFF) + Addr::from(self.x)) > 0xFF {
                    let dummy_addr = (addr0 & 0xFF00) | (addr & 0xFF);
                    self.readb(dummy_addr);
                }
                if addr0 == 0x2000 && self.x == 0x7 {
                    self.readb(addr);
                }
                let val = if read {
                    Word::from(self.readb(addr))
                } else {
                    0
                };
                let page_crossed = Cpu::pages_differ(addr0, addr);
                (val, Some(addr), 2, page_crossed)
            }
            AbsoluteY => {
                let addr0 = self.mem.readw(addr);
                let addr = addr0.wrapping_add(Addr::from(self.y));
                // dummy ST* read
                if !read && addr == 0x2007 {
                    let dummy_addr = (addr0 & 0xFF00) | (addr & 0xFF);
                    self.readb(dummy_addr);
                }
                let val = if read {
                    Word::from(self.readb(addr))
                } else {
                    0
                };
                let page_crossed = Cpu::pages_differ(addr0, addr);
                (val, Some(addr), 2, page_crossed)
            }
            Indirect => {
                let addr0 = self.mem.readw(addr);
                let addr = self.mem.readw_pagewrap(addr0);
                (0, Some(addr), 2, false)
            }
            IndirectX => {
                let addr_zp = self.readb(addr).wrapping_add(self.x);
                let addr = self.mem.readw_zp(addr_zp);
                let val = if read {
                    Word::from(self.readb(addr))
                } else {
                    0
                };
                (val, Some(addr), 1, false)
            }
            IndirectY => {
                let addr_zp = self.readb(addr);
                let addr_zp = self.mem.readw_zp(addr_zp);
                let addr = addr_zp.wrapping_add(Addr::from(self.y));
                // dummy read
                if Addr::from(addr_zp & 0xFF) + Addr::from(self.y) > 0xFF {
                    let dummy_addr = (addr_zp & 0xFF00) | (addr & 0xFF);
                    self.readb(dummy_addr);
                }
                let val = if read {
                    Word::from(self.readb(addr))
                } else {
                    0
                };
                let page_crossed = Cpu::pages_differ(addr_zp, addr);
                (val, Some(addr), 1, page_crossed)
            }
            Relative => {
                let val = if read {
                    Word::from(self.readb(addr))
                } else {
                    0
                };
                (val, Some(addr), 1, false)
            }
            Accumulator => {
                let _ = self.readb(addr); // dummy read
                (Word::from(self.acc), None, 0, false)
            }
            Implied => {
                let _ = self.readb(addr); // dummy read
                (0, None, 0, false)
            }
        }
    }

    // Reads from either a target address or the accumulator register.
    //
    // target is either Some(Addr) or None based on the addressing mode
    fn read_target(&mut self, target: Option<u16>) -> Byte {
        match target {
            None => self.acc,
            Some(addr) => self.readb(addr),
        }
    }

    // Reads from either a target address or the accumulator register.
    //
    // target is either Some(Addr) or None based on the addressing mode
    fn write_target(&mut self, target: Option<Addr>, val: Byte) {
        match target {
            None => {
                self.acc = val;
            }
            Some(addr) => self.writeb(addr, val),
        }
    }

    // Copies data to the PPU OAMDATA ($2004) using DMA (Direct Memory Access)
    // http://wiki.nesdev.com/w/index.php/PPU_registers#OAMDMA
    fn write_oamdma(&mut self, addr: Byte) {
        let mut addr = Addr::from(addr) << 8; // Start at $XX00
        let oam_addr = 0x2004;
        for i in 0..256 {
            // Copy 256 bytes from $XX00-$XXFF
            let val = self.readb(addr);
            self.writeb(oam_addr, val);
            addr = addr.saturating_add(1);
        }
        self.stall += 513; // +2 for every read/write and +1 dummy cycle
        if self.cycles % 2 == 1 {
            // +1 cycle if on an odd cycle
            self.stall += 1;
        }
    }

    // Print the current instruction and status
    fn print_instruction(&mut self, op: Operation, opcode: Byte, num_args: u8) {
        let word1 = if num_args < 2 {
            "  ".to_string()
        } else {
            format!("{:02X}", self.readb(self.pc.wrapping_add(1)))
        };
        let word2 = if num_args < 3 {
            "  ".to_string()
        } else {
            format!("{:02X}", self.readb(self.pc.wrapping_add(2)))
        };
        let asterisk = match op {
            NOP if opcode != 0xEA => "*",
            SBC if opcode == 0xEB => "*",
            DCP | ISB | LAX | RLA | RRA | SAX | SLO | SRE => "*",
            _ => " ",
        };
        let opstr = format!(
            "{:04X}  {:02X} {} {} {}{:29?} A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X} CYC:{}\n",
            self.pc,
            opcode,
            word1,
            word2,
            asterisk,
            op,
            self.acc,
            self.x,
            self.y,
            self.status,
            self.sp,
            self.cycles,
        );
        self.oplog.push_str(&opstr);
    }

    // Determines if address a and address b are on different pages
    fn pages_differ(a: u16, b: u16) -> bool {
        a & 0xFF00 != b & 0xFF00
    }
}

impl Memory for Cpu {
    fn readb(&mut self, addr: Addr) -> Byte {
        self.mem.readb(addr)
    }

    fn writeb(&mut self, addr: Addr, val: Byte) {
        if addr == 0x4014 {
            self.write_oamdma(val);
        } else {
            self.mem.writeb(addr, val);
        }
    }
}

#[rustfmt::skip]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
// List of all CPU official and unofficial operations
// http://wiki.nesdev.com/w/index.php/6502_instructions
enum Operation {
    ADC, AND, ASL, BCC, BCS, BEQ, BIT, BMI, BNE, BPL, BRK, BVC, BVS, CLC, CLD, CLI, CLV, CMP, CPX,
    CPY, DEC, DEX, DEY, EOR, INC, INX, INY, JMP, JSR, LDA, LDX, LDY, LSR, NOP, ORA, PHA, PHP, PLA,
    PLP, ROL, ROR, RTI, RTS, SBC, SEC, SED, SEI, STA, STX, STY, TAX, TAY, TSX, TXA, TXS, TYA,
    // "Unofficial" opcodes
    KIL, ISB, DCP, AXS, LAS, LAX, AHX, SAX, XAA, SHX, RRA, TAS, SHY, ARR, SRE, ALR, RLA, ANC, SLO,
}

#[rustfmt::skip]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
// List of all addressing modes
enum AddrMode {
    Immediate,
    ZeroPage, ZeroPageX, ZeroPageY,
    Absolute, AbsoluteX, AbsoluteY,
    Indirect, IndirectX, IndirectY,
    Relative,
    Accumulator,
    Implied,
}

use AddrMode::*;
use Operation::*;

type CycleCount = u64;
const IMM: AddrMode = Immediate;
const ZRP: AddrMode = ZeroPage;
const ZRX: AddrMode = ZeroPageX;
const ZRY: AddrMode = ZeroPageY;
const ABS: AddrMode = Absolute;
const ABX: AddrMode = AbsoluteX;
const ABY: AddrMode = AbsoluteY;
const IND: AddrMode = Indirect;
const IDX: AddrMode = IndirectX;
const IDY: AddrMode = IndirectY;
const REL: AddrMode = Relative;
const ACC: AddrMode = Accumulator;
const IMP: AddrMode = Implied;

// CPU Opcodes
//
// (Operation, Addressing Mode, Clock Cycles, Extra Cycles if page boundary is crossed)
#[rustfmt::skip]
const OPCODES: [(Operation, AddrMode, CycleCount, CycleCount); 256] = [
    (BRK, IMM, 7, 0), (ORA, IDX, 6, 0), (KIL, IMP, 0, 0), (SLO, IDX, 8, 0), // $00 - $03
    (NOP, ZRP, 3, 0), (ORA, ZRP, 3, 0), (ASL, ZRP, 5, 0), (SLO, ZRP, 5, 0), // $04 - $07
    (PHP, IMP, 3, 0), (ORA, IMM, 2, 0), (ASL, ACC, 2, 0), (ANC, IMM, 2, 0), // $08 - $0B
    (NOP, ABS, 4, 0), (ORA, ABS, 4, 0), (ASL, ABS, 6, 0), (SLO, ABS, 6, 0), // $0C - $0F
    (BPL, REL, 2, 1), (ORA, IDY, 5, 1), (KIL, IMP, 0, 0), (SLO, IDY, 8, 0), // $10 - $13
    (NOP, ZRX, 4, 0), (ORA, ZRX, 4, 0), (ASL, ZRX, 6, 0), (SLO, ZRX, 6, 0), // $14 - $17
    (CLC, IMP, 2, 0), (ORA, ABY, 4, 1), (NOP, IMP, 2, 0), (SLO, ABY, 7, 0), // $18 - $1B
    (NOP, ABX, 4, 1), (ORA, ABX, 4, 1), (ASL, ABX, 7, 0), (SLO, ABX, 7, 0), // $1C - $1F
    (JSR, ABS, 6, 0), (AND, IDX, 6, 0), (KIL, IMP, 0, 0), (RLA, IDX, 8, 0), // $20 - $23
    (BIT, ZRP, 3, 0), (AND, ZRP, 3, 0), (ROL, ZRP, 5, 0), (RLA, ZRP, 5, 0), // $24 - $27
    (PLP, IMP, 4, 0), (AND, IMM, 2, 0), (ROL, ACC, 2, 0), (ANC, IMM, 2, 0), // $28 - $2B
    (BIT, ABS, 4, 0), (AND, ABS, 4, 0), (ROL, ABS, 6, 0), (RLA, ABS, 6, 0), // $2C - $2F
    (BMI, REL, 2, 1), (AND, IDY, 5, 1), (KIL, IMP, 0, 0), (RLA, IDY, 8, 0), // $30 - $33
    (NOP, ZRX, 4, 0), (AND, ZRX, 4, 0), (ROL, ZRX, 6, 0), (RLA, ZRX, 6, 0), // $34 - $37
    (SEC, IMP, 2, 0), (AND, ABY, 4, 1), (NOP, IMP, 2, 0), (RLA, ABY, 7, 0), // $38 - $3B
    (NOP, ABX, 4, 1), (AND, ABX, 4, 1), (ROL, ABX, 7, 0), (RLA, ABX, 7, 0), // $3C - $3F
    (RTI, IMP, 6, 0), (EOR, IDX, 6, 0), (KIL, IMP, 0, 0), (SRE, IDX, 8, 0), // $40 - $43
    (NOP, ZRP, 3, 0), (EOR, ZRP, 3, 0), (LSR, ZRP, 5, 0), (SRE, ZRP, 5, 0), // $44 - $47
    (PHA, IMP, 3, 0), (EOR, IMM, 2, 0), (LSR, ACC, 2, 0), (ALR, IMM, 2, 0), // $48 - $4B
    (JMP, ABS, 3, 0), (EOR, ABS, 4, 0), (LSR, ABS, 6, 0), (SRE, ABS, 6, 0), // $4C - $4F
    (BVC, REL, 2, 1), (EOR, IDY, 5, 1), (KIL, IMP, 0, 0), (SRE, IDY, 8, 0), // $50 - $53
    (NOP, ZRX, 4, 0), (EOR, ZRX, 4, 0), (LSR, ZRX, 6, 0), (SRE, ZRX, 6, 0), // $54 - $57
    (CLI, IMP, 2, 0), (EOR, ABY, 4, 1), (NOP, IMP, 2, 0), (SRE, ABY, 7, 0), // $58 - $5B
    (NOP, ABX, 4, 1), (EOR, ABX, 4, 1), (LSR, ABX, 7, 0), (SRE, ABX, 7, 0), // $5C - $5F
    (RTS, IMP, 6, 0), (ADC, IDX, 6, 0), (KIL, IMP, 0, 0), (RRA, IDX, 8, 0), // $60 - $63
    (NOP, ZRP, 3, 0), (ADC, ZRP, 3, 0), (ROR, ZRP, 5, 0), (RRA, ZRP, 5, 0), // $64 - $67
    (PLA, IMP, 4, 0), (ADC, IMM, 2, 0), (ROR, ACC, 2, 0), (ARR, IMM, 2, 0), // $68 - $6B
    (JMP, IND, 5, 0), (ADC, ABS, 4, 0), (ROR, ABS, 6, 0), (RRA, ABS, 6, 0), // $6C - $6F
    (BVS, REL, 2, 1), (ADC, IDY, 5, 1), (KIL, IMP, 0, 0), (RRA, IDY, 8, 0), // $70 - $73
    (NOP, ZRX, 4, 0), (ADC, ZRX, 4, 0), (ROR, ZRX, 6, 0), (RRA, ZRX, 6, 0), // $74 - $77
    (SEI, IMP, 2, 0), (ADC, ABY, 4, 1), (NOP, IMP, 2, 0), (RRA, ABY, 7, 0), // $78 - $7B
    (NOP, ABX, 4, 1), (ADC, ABX, 4, 1), (ROR, ABX, 7, 0), (RRA, ABX, 7, 0), // $7C - $7F
    (NOP, IMM, 2, 0), (STA, IDX, 6, 0), (NOP, IMM, 2, 0), (SAX, IDX, 6, 0), // $80 - $83
    (STY, ZRP, 3, 0), (STA, ZRP, 3, 0), (STX, ZRP, 3, 0), (SAX, ZRP, 3, 0), // $84 - $87
    (DEY, IMP, 2, 0), (NOP, IMM, 2, 0), (TXA, IMP, 2, 0), (XAA, IMM, 2, 1), // $88 - $8B
    (STY, ABS, 4, 0), (STA, ABS, 4, 0), (STX, ABS, 4, 0), (SAX, ABS, 4, 0), // $8C - $8F
    (BCC, REL, 2, 1), (STA, IDY, 6, 0), (KIL, IMP, 0, 0), (AHX, IDY, 6, 0), // $90 - $93
    (STY, ZRX, 4, 0), (STA, ZRX, 4, 0), (STX, ZRY, 4, 0), (SAX, ZRY, 4, 0), // $94 - $97
    (TYA, IMP, 2, 0), (STA, ABY, 5, 0), (TXS, IMP, 2, 0), (TAS, ABY, 5, 0), // $98 - $9B
    (SHY, ABX, 5, 0), (STA, ABX, 5, 0), (SHX, ABY, 5, 0), (AHX, ABY, 5, 0), // $9C - $9F
    (LDY, IMM, 2, 0), (LDA, IDX, 6, 0), (LDX, IMM, 2, 0), (LAX, IDX, 6, 0), // $A0 - $A3
    (LDY, ZRP, 3, 0), (LDA, ZRP, 3, 0), (LDX, ZRP, 3, 0), (LAX, ZRP, 3, 0), // $A4 - $A7
    (TAY, IMP, 2, 0), (LDA, IMM, 2, 0), (TAX, IMP, 2, 0), (LAX, IMM, 2, 0), // $A8 - $AB
    (LDY, ABS, 4, 0), (LDA, ABS, 4, 0), (LDX, ABS, 4, 0), (LAX, ABS, 4, 0), // $AC - $AF
    (BCS, REL, 2, 1), (LDA, IDY, 5, 1), (KIL, IMP, 0, 0), (LAX, IDY, 5, 1), // $B0 - $B3
    (LDY, ZRX, 4, 0), (LDA, ZRX, 4, 0), (LDX, ZRY, 4, 0), (LAX, ZRY, 4, 0), // $B4 - $B7
    (CLV, IMP, 2, 0), (LDA, ABY, 4, 1), (TSX, IMP, 2, 0), (LAS, ABY, 4, 1), // $B8 - $BB
    (LDY, ABX, 4, 1), (LDA, ABX, 4, 1), (LDX, ABY, 4, 1), (LAX, ABY, 4, 1), // $BC - $BF
    (CPY, IMM, 2, 0), (CMP, IDX, 6, 0), (NOP, IMM, 2, 0), (DCP, IDX, 8, 0), // $C0 - $C3
    (CPY, ZRP, 3, 0), (CMP, ZRP, 3, 0), (DEC, ZRP, 5, 0), (DCP, ZRP, 5, 0), // $C4 - $C7
    (INY, IMP, 2, 0), (CMP, IMM, 2, 0), (DEX, IMP, 2, 0), (AXS, IMM, 2, 0), // $C8 - $CB
    (CPY, ABS, 4, 0), (CMP, ABS, 4, 0), (DEC, ABS, 6, 0), (DCP, ABS, 6, 0), // $CC - $CF
    (BNE, REL, 2, 1), (CMP, IDY, 5, 1), (KIL, IMP, 0, 0), (DCP, IDY, 8, 0), // $D0 - $D3
    (NOP, ZRX, 4, 0), (CMP, ZRX, 4, 0), (DEC, ZRX, 6, 0), (DCP, ZRX, 6, 0), // $D4 - $D7
    (CLD, IMP, 2, 0), (CMP, ABY, 4, 1), (NOP, IMP, 2, 0), (DCP, ABY, 7, 0), // $D8 - $DB
    (NOP, ABX, 4, 1), (CMP, ABX, 4, 1), (DEC, ABX, 7, 0), (DCP, ABX, 7, 0), // $DC - $DF
    (CPX, IMM, 2, 0), (SBC, IDX, 6, 0), (NOP, IMM, 2, 0), (ISB, IDX, 8, 0), // $E0 - $E3
    (CPX, ZRP, 3, 0), (SBC, ZRP, 3, 0), (INC, ZRP, 5, 0), (ISB, ZRP, 5, 0), // $E4 - $E7
    (INX, IMP, 2, 0), (SBC, IMM, 2, 0), (NOP, IMP, 2, 0), (SBC, IMM, 2, 0), // $E8 - $EB
    (CPX, ABS, 4, 0), (SBC, ABS, 4, 0), (INC, ABS, 6, 0), (ISB, ABS, 6, 0), // $EC - $EF
    (BEQ, REL, 2, 1), (SBC, IDY, 5, 1), (KIL, IMP, 0, 0), (ISB, IDY, 8, 0), // $F0 - $F3
    (NOP, ZRX, 4, 0), (SBC, ZRX, 4, 0), (INC, ZRX, 6, 0), (ISB, ZRX, 6, 0), // $F4 - $F7
    (SED, IMP, 2, 0), (SBC, ABY, 4, 1), (NOP, IMP, 2, 0), (ISB, ABY, 7, 0), // $F8 - $FB
    (NOP, ABX, 4, 1), (SBC, ABX, 4, 1), (INC, ABX, 7, 0), (ISB, ABX, 7, 0), // $FC - $FF
];

impl Cpu {
    // Storage opcodes

    // LDA: Load A with M
    fn lda(&mut self, val: Byte) {
        self.acc = val;
        self.update_acc();
    }
    // LDX: Load X with M
    fn ldx(&mut self, val: Byte) {
        self.x = val;
        self.set_result_flags(val);
    }
    // LDY: Load Y with M
    fn ldy(&mut self, val: Byte) {
        self.y = val;
        self.set_result_flags(val);
    }
    // TAX: Transfer A to X
    fn tax(&mut self) {
        self.x = self.acc;
        self.set_result_flags(self.x);
    }
    // TAY: Transfer A to Y
    fn tay(&mut self) {
        self.y = self.acc;
        self.set_result_flags(self.y);
    }
    // TSX: Transfer Stack Pointer to X
    fn tsx(&mut self) {
        self.x = self.sp;
        self.set_result_flags(self.x);
    }
    // TXA: Transfer X to A
    fn txa(&mut self) {
        self.acc = self.x;
        self.update_acc();
    }
    // TXS: Transfer X to Stack Pointer
    fn txs(&mut self) {
        self.sp = self.x;
    }
    // TYA: Transfer Y to A
    fn tya(&mut self) {
        self.acc = self.y;
        self.update_acc();
    }

    // Arithmetic opcodes

    // ADC: Add M to A with Carry
    fn adc(&mut self, val: Byte) {
        let a = self.acc;
        let (x1, o1) = val.overflowing_add(a);
        let (x2, o2) = x1.overflowing_add(self.carry() as Byte);
        self.acc = x2;
        self.set_carry(o1 | o2);
        self.set_overflow((a ^ val) & 0x80 == 0 && (a ^ self.acc) & 0x80 != 0);
        self.update_acc();
    }
    // SBC: Subtract M from A with Carry
    fn sbc(&mut self, val: Byte) {
        let a = self.acc;
        let (x1, o1) = a.overflowing_sub(val);
        let (x2, o2) = x1.overflowing_sub(1 - self.carry() as Byte);
        self.acc = x2;
        self.set_carry(!(o1 | o2));
        self.set_overflow((a ^ val) & 0x80 != 0 && (a ^ self.acc) & 0x80 != 0);
        self.update_acc();
    }
    // DEC: Decrement M by One
    fn dec(&mut self, target: Option<Addr>) {
        let val = self.read_target(target);
        self.write_target(target, val); // dummy write
        let val = val.wrapping_sub(1);
        self.set_result_flags(val);
        self.write_target(target, val);
    }
    // DEX: Decrement X by One
    fn dex(&mut self) {
        self.x = self.x.wrapping_sub(1);
        self.set_result_flags(self.x);
    }
    // DEY: Decrement Y by One
    fn dey(&mut self) {
        self.y = self.y.wrapping_sub(1);
        self.set_result_flags(self.y);
    }
    // INC: Increment M by One
    fn inc(&mut self, target: Option<Addr>) {
        let val = self.read_target(target);
        self.write_target(target, val); // dummy write
        let val = val.wrapping_add(1);
        self.set_result_flags(val);
        self.write_target(target, val);
    }
    // INX: Increment X by One
    fn inx(&mut self) {
        self.x = self.x.wrapping_add(1);
        self.set_result_flags(self.x);
    }
    // INY: Increment Y by One
    fn iny(&mut self) {
        self.y = self.y.wrapping_add(1);
        self.set_result_flags(self.y);
    }

    // Bitwise opcodes

    // AND: "And" M with A
    fn and(&mut self, val: Byte) {
        self.acc &= val;
        self.update_acc();
    }
    // ASL: Shift Left One Bit (M or A)
    fn asl(&mut self, target: Option<Addr>) {
        let val = self.read_target(target);
        self.write_target(target, val);
        self.set_carry((val >> 7) & 1 > 0);
        let val = val.wrapping_shl(1);
        self.set_result_flags(val);
        self.write_target(target, val);
    }
    // BIT: Test Bits in M with A (Affects N, V, and Z)
    fn bit(&mut self, val: Byte) {
        self.set_overflow((val >> 6) & 1 > 0);
        self.set_zero((val & self.acc) == 0);
        self.set_negative(is_negative(val));
    }
    // EOR: "Exclusive-Or" M with A
    fn eor(&mut self, val: Byte) {
        self.acc ^= val;
        self.update_acc();
    }
    // LSR: Shift Right One Bit (M or A)
    fn lsr(&mut self, target: Option<Addr>) {
        let val = self.read_target(target);
        self.write_target(target, val);
        self.set_carry(val & 1 > 0);
        let val = val.wrapping_shr(1);
        self.set_result_flags(val);
        self.write_target(target, val);
    }
    // ORA: "OR" M with A
    fn ora(&mut self, val: Byte) {
        self.acc |= val;
        self.update_acc();
    }
    // ROL: Rotate One Bit Left (M or A)
    fn rol(&mut self, target: Option<Addr>) {
        let val = self.read_target(target);
        self.write_target(target, val); // dummy write
        let old_c = self.carry() as Byte;
        self.set_carry((val >> 7) & 1 > 0);
        let val = (val << 1) | old_c;
        self.set_result_flags(val);
        self.write_target(target, val);
    }
    // ROR: Rotate One Bit Right (M or A)
    fn ror(&mut self, target: Option<Addr>) {
        let val = self.read_target(target);
        self.write_target(target, val);
        let mut ret = val.rotate_right(1);
        if self.carry() {
            ret |= 1 << 7;
        } else {
            ret &= !(1 << 7);
        }
        self.set_carry(val & 1 > 0);
        self.set_result_flags(ret);
        self.write_target(target, ret);
    }

    // Branch opcodes

    // Utility function used by all branch instructions
    fn branch(&mut self, val: Byte) {
        let old_pc = self.pc;
        self.pc = self.pc.wrapping_add((val as i8) as Addr);
        self.cycles += 1;
        if Cpu::pages_differ(self.pc, old_pc) {
            self.cycles += 1;
        }
    }
    // BCC: Branch on Carry Clear
    fn bcc(&mut self, val: Byte) {
        if !self.carry() {
            self.branch(val);
        }
    }
    // BCS: Branch on Carry Set
    fn bcs(&mut self, val: Byte) {
        if self.carry() {
            self.branch(val);
        }
    }
    // BEQ: Branch on Result Zero
    fn beq(&mut self, val: Byte) {
        if self.zero() {
            self.branch(val);
        }
    }
    // BMI: Branch on Result Negative
    fn bmi(&mut self, val: Byte) {
        if self.negative() {
            self.branch(val);
        }
    }
    // BNE: Branch on Result Not Zero
    fn bne(&mut self, val: Byte) {
        if !self.zero() {
            self.branch(val);
        }
    }
    // BPL: Branch on Result Positive
    fn bpl(&mut self, val: Byte) {
        if !self.negative() {
            self.branch(val);
        }
    }
    // BVC: Branch on Overflow Clear
    fn bvc(&mut self, val: Byte) {
        if !self.overflow() {
            self.branch(val);
        }
    }
    // BVS: Branch on Overflow Set
    fn bvs(&mut self, val: Byte) {
        if self.overflow() {
            self.branch(val);
        }
    }

    // Jump opcodes

    // JMP: Jump to Location
    fn jmp(&mut self, addr: Addr) {
        self.pc = addr;
    }
    // JSR: Jump to Location Save Return addr
    fn jsr(&mut self, addr: Addr) {
        self.push_stackw(self.pc.wrapping_sub(1));
        self.pc = addr;
    }
    // RTI: Return from Interrupt
    fn rti(&mut self) {
        self.status = (self.pop_stackb() | UNUSED_FLAG) & !BREAK_FLAG;
        self.pc = self.pop_stackw();
    }
    // RTS: Return from Subroutine
    fn rts(&mut self) {
        self.pc = self.pop_stackw().wrapping_add(1);
    }

    // Register opcodes

    // CLC: Clear Carry Flag
    fn clc(&mut self) {
        self.set_carry(false);
    }
    // SEC: Set Carry Flag
    fn sec(&mut self) {
        self.set_carry(true);
    }
    // CLD: Clear Decimal Mode
    fn cld(&mut self) {
        self.set_decimal(false);
    }
    // SED: Set Decimal Mode
    fn sed(&mut self) {
        self.set_decimal(true);
    }
    // CLI: Clear Interrupt Disable Bit
    fn cli(&mut self) {
        self.set_interrupt_disable(false);
    }
    // SEI: Set Interrupt Disable Status
    fn sei(&mut self) {
        self.set_interrupt_disable(true);
    }
    // STA: Store A into M
    fn sta(&mut self, addr: Option<Addr>) {
        self.write_target(addr, self.acc);
    }
    // STX: Store X into M
    fn stx(&mut self, addr: Option<Addr>) {
        self.write_target(addr, self.x);
    }
    // STY: Store Y into M
    fn sty(&mut self, addr: Option<Addr>) {
        self.write_target(addr, self.y);
    }
    // CLV: Clear Overflow Flag
    fn clv(&mut self) {
        self.set_overflow(false);
    }

    // Compare opcodes

    // Utility function used by all compare instructions
    fn compare(&mut self, a: Byte, b: Byte) {
        let result = a.wrapping_sub(b);
        self.set_result_flags(result);
        self.set_carry(a >= b);
    }
    // CMP: Compare M and A
    fn cmp(&mut self, val: Byte) {
        let a = self.acc;
        self.compare(a, val);
    }
    // CPX: Compare M and X
    fn cpx(&mut self, val: Byte) {
        let x = self.x;
        self.compare(x, val);
    }
    // CPY: Compare M and Y
    fn cpy(&mut self, val: Byte) {
        let y = self.y;
        self.compare(y, val);
    }

    // Stack opcodes

    // PHP: Push Processor Status on Stack
    fn php(&mut self) {
        self.push_stackb(self.status | UNUSED_FLAG | BREAK_FLAG);
    }
    // PLP: Pull Processor Status from Stack
    fn plp(&mut self) {
        self.status = (self.pop_stackb() | UNUSED_FLAG) & !BREAK_FLAG;
    }
    // PHA: Push A on Stack
    fn pha(&mut self) {
        self.push_stackb(self.acc);
    }
    // PLA: Pull A from Stack
    fn pla(&mut self) {
        self.acc = self.pop_stackb();
        self.update_acc();
    }

    // System opcodes

    // BRK: Force Break Interrupt
    fn brk(&mut self) {
        self.push_stackw(self.pc);
        self.push_stackb(self.status | UNUSED_FLAG | BREAK_FLAG);
        self.php();
        self.sei();
        self.pc = self.mem.readw(IRQ_ADDR);
    }
    // NOP: No Operation
    fn nop(&mut self) {}

    // Unofficial opcodes

    fn kil(&self) {
        panic!("KIL encountered");
    }
    // ISC/ISB: Shortcut for INC then SBC
    fn isb(&mut self, target: Option<Addr>) {
        let val = self.read_target(target);
        self.write_target(target, val);
        let val = val.wrapping_add(1);
        self.set_result_flags(val);
        self.sbc(val);
        self.write_target(target, val);
    }
    // DCP: Shortcut for DEC then CMP
    fn dcp(&mut self, target: Option<Addr>) {
        let val = self.read_target(target);
        self.write_target(target, val);
        let val = val.wrapping_sub(1);
        self.compare(self.acc, val);
        self.write_target(target, val);
    }
    // AXS: A & X into X
    fn axs(&mut self) {
        self.x &= self.acc;
        self.set_result_flags(self.x);
    }
    // LAS: Shortcut for LDA then TSX
    fn las(&mut self, val: Byte) {
        self.lda(val);
        self.tsx();
    }
    // LAX: Shortcut for LDA then TAX
    fn lax(&mut self, val: Byte) {
        self.lda(val);
        self.tax();
    }
    // AHX: TODO
    fn ahx(&mut self) {
        unimplemented!();
    }
    // SAX: AND A with X
    fn sax(&mut self, target: Option<Addr>) {
        let val = self.acc & self.x;
        self.write_target(target, val);
    }
    // XAA: TODO
    fn xaa(&mut self) {
        unimplemented!();
    }
    // SHX: TODO
    fn shx(&mut self) {
        unimplemented!();
    }
    // RRA: Shortcut for ROR then ADC
    fn rra(&mut self, target: Option<Addr>) {
        self.ror(target);
        let val = self.read_target(target);
        self.adc(val);
    }
    // TAS: Shortcut for STA then TXS
    fn tas(&mut self, addr: Option<Addr>) {
        self.sta(addr);
        self.txs();
    }
    // SHY: TODO
    fn shy(&mut self) {
        unimplemented!();
    }
    // ARR: TODO
    fn arr(&mut self) {
        unimplemented!();
    }
    // SRA: Shortcut for LSR then EOR
    fn sre(&mut self, target: Option<Addr>) {
        self.lsr(target);
        let val = self.read_target(target);
        self.eor(val);
    }
    // ALR: TODO
    fn alr(&mut self) {
        unimplemented!();
    }
    // RLA: Shortcut for ROL then AND
    fn rla(&mut self, target: Option<Addr>) {
        self.rol(target);
        let val = self.read_target(target);
        self.and(val);
    }
    // anc: TODO
    fn anc(&mut self) {
        unimplemented!();
    }
    // SLO: Shortcut for ASL then ORA
    fn slo(&mut self, target: Option<Addr>) {
        self.asl(target);
        let val = self.read_target(target);
        self.ora(val);
    }
}

// Since we're working with u8s, we need a way to check for negative numbers
fn is_negative(val: Byte) -> bool {
    val >= 128
}

impl fmt::Debug for Cpu {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "CPU {{ {:04X} A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X} CYC:{} }}",
            self.pc, self.acc, self.x, self.y, self.status, self.sp, self.cycles,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::cartridge::Cartridge;
    use crate::console::input::Input;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    const TEST_ROM: &str = "tests/cpu/nestest.nes";
    const TEST_PC: Addr = 49156;

    #[test]
    fn test_cpu_new() {
        let rom = &PathBuf::from(TEST_ROM);
        let board = Cartridge::new(rom)
            .expect("cartridge")
            .load_board()
            .expect("loaded board");
        let input = Rc::new(RefCell::new(Input::new()));
        let mut cpu_memory = CpuMemMap::init(board, input);
        let c = Cpu::init(cpu_memory);
        assert_eq!(c.cycles, 7);
        assert_eq!(c.pc, TEST_PC);
        assert_eq!(c.sp, POWER_ON_SP);
        assert_eq!(c.acc, 0);
        assert_eq!(c.x, 0);
        assert_eq!(c.y, 0);
        assert_eq!(c.status, POWER_ON_STATUS);
    }

    #[test]
    fn test_cpu_reset() {
        let rom = &PathBuf::from(TEST_ROM);
        let board = Cartridge::new(rom)
            .expect("cartridge")
            .load_board()
            .expect("loaded board");
        let input = Rc::new(RefCell::new(Input::new()));
        let mut cpu_memory = CpuMemMap::init(board, input);
        let mut c = Cpu::init(cpu_memory);
        c.reset();
        assert_eq!(c.pc, TEST_PC);
        assert_eq!(c.sp, POWER_ON_SP - 3);
        assert_eq!(c.status, POWER_ON_STATUS);
        assert_eq!(c.cycles, 7);
    }
}
