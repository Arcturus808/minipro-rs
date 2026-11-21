//! RAR virtual-machine filters: program registry, filter stack, and the four
//! built-in filter programs (delta, x86 E8/E9, RGB, audio).
//!
//! Port of the filter sections of `archive_read_support_format_rar.c`:
//! `parse_filter`, `create_filter`, `compile_program`, `execute_filter_*`,
//! `membr_*`, `read_filter`.

use crate::crc32::crc32;
use crate::error::Error;
use crate::stream::Stream;
use std::collections::VecDeque;
use std::ops::Deref;

pub(crate) const VM_MEMORY_SIZE: usize = 0x40000;
pub(crate) const PROGRAM_WORK_SIZE: u32 = 0x3C000;
pub(crate) const PROGRAM_GLOBAL_SIZE: u32 = 0x2000;
pub(crate) const PROGRAM_SYSTEM_GLOBAL_ADDRESS: u32 = PROGRAM_WORK_SIZE;
pub(crate) const PROGRAM_SYSTEM_GLOBAL_SIZE: u32 = 0x40;
pub(crate) const PROGRAM_USER_GLOBAL_SIZE: u32 = PROGRAM_GLOBAL_SIZE - PROGRAM_SYSTEM_GLOBAL_SIZE;

/// `struct memory_bit_reader`: bit reader over a byte slice, MSB first.
struct MemoryBitReader<'a> {
    bytes: &'a [u8],
    offset: usize,
    bits: u64,
    available: i32,
    at_eof: bool,
}

impl<'a> MemoryBitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            offset: 0,
            bits: 0,
            available: 0,
            at_eof: false,
        }
    }

    fn fill(&mut self, bits: i32) -> bool {
        while self.available < bits && self.offset < self.bytes.len() {
            self.bits = (self.bits << 8) | self.bytes[self.offset] as u64;
            self.offset += 1;
            self.available += 8;
        }
        if bits > self.available {
            self.at_eof = true;
            return false;
        }
        true
    }

    fn bits(&mut self, bits: i32) -> u32 {
        if bits > self.available && (self.at_eof || !self.fill(bits)) {
            return 0;
        }
        self.available -= bits;
        ((self.bits >> self.available) & ((1u64 << bits) - 1)) as u32
    }

    /// `membr_next_rarvm_number`: RAR VM variable-length integer.
    fn next_rarvm_number(&mut self) -> u32 {
        match self.bits(2) {
            0 => self.bits(4),
            1 => {
                let val = self.bits(8);
                if val >= 16 {
                    val
                } else {
                    0xFFFF_FF00 | (val << 4) | self.bits(4)
                }
            }
            2 => self.bits(16),
            _ => self.bits(32),
        }
    }
}

/// `struct rar_program_code`: a registered VM program (recognized by
/// fingerprint — only the four built-ins are supported).
#[derive(Debug)]
pub(crate) struct ProgramCode {
    pub staticdata: Option<Vec<u8>>,
    pub fingerprint: u64,
    pub usagecount: u32,
    pub oldfilterlength: u32,
}

/// `struct rar_filter`.
#[derive(Debug)]
pub(crate) struct RarFilter {
    /// Index into [`RarFilters::progs`].
    pub prog: usize,
    pub initialregisters: [u32; 8],
    pub globaldata: Vec<u8>,
    pub blockstartpos: usize,
    pub blocklength: u32,
    pub filteredblockaddress: u32,
    pub filteredblocklength: u32,
}

/// `struct rar_virtual_machine`. The register file is omitted: the only
/// programs this reader can execute are the four built-ins, which read
/// `RarFilter::initialregisters` directly.
#[derive(Debug)]
pub(crate) struct VirtualMachine {
    pub memory: Vec<u8>,
}

impl VirtualMachine {
    pub(crate) fn new() -> Self {
        Self {
            memory: vec![0; VM_MEMORY_SIZE + 4],
        }
    }

    #[inline]
    pub fn read_32(&self, offset: usize) -> u32 {
        u32::from_le_bytes([
            self.memory[offset],
            self.memory[offset + 1],
            self.memory[offset + 2],
            self.memory[offset + 3],
        ])
    }

    #[inline]
    pub fn write_32(&mut self, offset: usize, v: u32) {
        self.memory[offset..offset + 4].copy_from_slice(&v.to_le_bytes());
    }
}

/// `struct rar_filters`.
#[derive(Debug, Default)]
pub(crate) struct RarFilters {
    pub vm: Option<Box<VirtualMachine>>,
    pub progs: Vec<ProgramCode>,
    /// FIFO of pending filters (the C `stack` list is appended at the tail).
    pub stack: VecDeque<RarFilter>,
    /// Absolute LZSS position where the next filter block starts
    /// (`INT64_MAX` when none is pending).
    pub filterstart: i64,
    pub lastfilternum: u32,
    /// End position of the last run/expanded block.
    pub lastend: i64,
    /// Filtered output bytes pending emission (`filters->bytes/bytes_ready`).
    pub out: Vec<u8>,
    pub out_pos: usize,
    pub bytes_ready: usize,
    /// Bumped whenever the pending-filter stack is reinitialized; used by
    /// `run_filters` to detect `delete_filter(filters->stack)` mid-expand
    /// (the C pointer-identity check).
    pub generation: u64,
}

impl RarFilters {
    pub fn new() -> Self {
        Self {
            filterstart: i64::MAX,
            ..Self::default()
        }
    }

    /// `parse_filter`: register a filter from its encoded description.
    /// `lzss_position`/`dictionary_size`/`lzss_mask` describe the current
    /// decode state.
    pub fn parse_filter(
        &mut self,
        bytes: &[u8],
        flags: u8,
        lzss_position: i64,
        dictionary_size: u32,
        lzss_mask: usize,
    ) -> bool {
        let mut br = MemoryBitReader::new(bytes);

        let numprogs = self.progs.len() as u32;
        let num;
        if flags & 0x80 != 0 {
            let mut n = br.next_rarvm_number();
            if n == 0 {
                self.stack.clear();
                self.progs.clear();
                self.generation += 1;
            } else {
                n -= 1;
            }
            if n > numprogs {
                return false;
            }
            num = n;
            self.lastfilternum = num;
        } else {
            num = self.lastfilternum;
        }

        let mut prog_idx = if (num as usize) < self.progs.len() {
            Some(num as usize)
        } else {
            None
        };
        if let Some(i) = prog_idx {
            self.progs[i].usagecount += 1;
        }

        let mut blockstartpos = br.next_rarvm_number() as usize + lzss_position as usize;
        if flags & 0x40 != 0 {
            blockstartpos += 258;
        }
        let blocklength = if flags & 0x20 != 0 {
            br.next_rarvm_number()
        } else {
            match prog_idx {
                Some(i) => self.progs[i].oldfilterlength,
                None => 0,
            }
        };

        if blocklength > dictionary_size || blocklength as usize > lzss_mask + 1 {
            return false;
        }

        let mut registers = [0u32; 8];
        registers[3] = PROGRAM_SYSTEM_GLOBAL_ADDRESS;
        registers[4] = blocklength;
        registers[5] = prog_idx.map_or(0, |i| self.progs[i].usagecount);
        registers[7] = VM_MEMORY_SIZE as u32;

        if flags & 0x10 != 0 {
            let mask = br.bits(7) as u8;
            for (i, r) in registers.iter_mut().enumerate().take(7) {
                if mask & (1 << i) != 0 {
                    *r = br.next_rarvm_number();
                }
            }
        }

        if prog_idx.is_none() {
            let len = br.next_rarvm_number();
            if len == 0 || len > 0x10000 {
                return false;
            }
            let mut bytecode = vec![0u8; len as usize];
            for b in bytecode.iter_mut() {
                *b = br.bits(8) as u8;
            }
            match compile_program(&bytecode) {
                Some(prog) => {
                    self.progs.push(prog);
                    prog_idx = Some(self.progs.len() - 1);
                }
                None => return false,
            }
        }
        let pi = prog_idx.unwrap();
        self.progs[pi].oldfilterlength = blocklength;

        let mut globaldata: Vec<u8> = Vec::new();
        let mut globaldatalen = 0u32;
        if flags & 0x08 != 0 {
            globaldatalen = br.next_rarvm_number();
            if globaldatalen > PROGRAM_USER_GLOBAL_SIZE {
                return false;
            }
            globaldata = vec![0u8; (globaldatalen + PROGRAM_SYSTEM_GLOBAL_SIZE) as usize];
            for i in 0..globaldatalen as usize {
                globaldata[i + PROGRAM_SYSTEM_GLOBAL_SIZE as usize] = br.bits(8) as u8;
            }
        }

        if br.at_eof {
            return false;
        }

        // create_filter
        let gdlen = globaldatalen.max(PROGRAM_SYSTEM_GLOBAL_SIZE) as usize;
        let mut gdata = vec![0u8; gdlen];
        let copy = globaldatalen.min(globaldata.len() as u32) as usize;
        gdata[..copy].copy_from_slice(&globaldata[..copy]);
        let mut filter = RarFilter {
            prog: pi,
            initialregisters: registers,
            globaldata: gdata,
            blockstartpos,
            blocklength,
            filteredblockaddress: 0,
            filteredblocklength: 0,
        };
        for (i, r) in registers.iter().enumerate().take(7) {
            filter.globaldata[i * 4..i * 4 + 4].copy_from_slice(&r.to_le_bytes());
        }
        filter.globaldata[0x1C..0x20].copy_from_slice(&blocklength.to_le_bytes());
        filter.globaldata[0x20..0x24].copy_from_slice(&0u32.to_le_bytes());
        filter.globaldata[0x2C..0x30].copy_from_slice(&self.progs[pi].usagecount.to_le_bytes());

        let was_empty = self.stack.is_empty();
        self.stack.push_back(filter);
        if was_empty {
            self.filterstart = blockstartpos as i64;
        }
        true
    }
}

/// `compile_program`: verify the xor checksum and read optional static data.
fn compile_program(bytes: &[u8]) -> Option<ProgramCode> {
    let mut xor = 0u8;
    for &b in &bytes[1..] {
        xor ^= b;
    }
    if bytes.is_empty() || xor != bytes[0] {
        return None;
    }

    let mut br = MemoryBitReader::new(bytes);
    br.offset = 1;

    let mut prog = ProgramCode {
        staticdata: None,
        fingerprint: crc32(0, bytes) as u64 | ((bytes.len() as u64) << 32),
        usagecount: 0,
        oldfilterlength: 0,
    };

    if br.bits(1) != 0 {
        let staticdatalen = br.next_rarvm_number();
        if staticdatalen as usize >= VM_MEMORY_SIZE {
            return None;
        }
        let len = staticdatalen + 1;
        let mut data = vec![0u8; len as usize];
        for b in data.iter_mut() {
            *b = br.bits(8) as u8;
        }
        prog.staticdata = Some(data);
    }
    Some(prog)
}

/// `read_filter`: decode the filter descriptor following symbol 257.
/// Returns false on bad data.
pub(crate) fn read_filter<B: Deref<Target = [u8]>>(
    stream: &mut Stream<B>,
    filters: &mut RarFilters,
    lzss_position: i64,
    dictionary_size: u32,
    lzss_mask: usize,
    end: &mut i64,
) -> bool {
    let Some(flags) = stream.br_pull_byte() else {
        return false;
    };
    let mut length = (flags & 0x07) as u16 + 1;
    if length == 7 {
        let Some(val) = stream.br_pull_byte() else {
            return false;
        };
        length = val as u16 + 7;
    } else if length == 8 {
        let Some(v1) = stream.br_pull_byte() else {
            return false;
        };
        let Some(v2) = stream.br_pull_byte() else {
            return false;
        };
        length = ((v1 as u16) << 8) | v2 as u16;
    }

    let mut code = vec![0u8; length as usize];
    for c in code.iter_mut() {
        let Some(b) = stream.br_pull_byte() else {
            return false;
        };
        *c = b;
    }
    if !filters.parse_filter(&code, flags, lzss_position, dictionary_size, lzss_mask) {
        return false;
    }

    if filters.filterstart < *end {
        *end = filters.filterstart;
    }
    true
}

// ---- built-in filter programs ----

#[derive(Debug, Default)]
struct AudioState {
    weight: [i8; 5],
    delta: [i8; 4],
    lastdelta: i8,
    error: [i32; 11],
    count: u32,
    lastbyte: u8,
}

fn execute_filter_delta(filter: &mut RarFilter, vm: &mut VirtualMachine) -> bool {
    let length = filter.initialregisters[4] as usize;
    let numchannels = filter.initialregisters[0] as usize;

    if length as u32 > PROGRAM_WORK_SIZE / 2 || numchannels == 0 || numchannels > 128 {
        return false;
    }

    let mut src = 0usize;
    let dst = length;
    for i in 0..numchannels {
        let mut lastbyte = 0u8;
        let mut idx = i;
        while idx < length {
            if src >= dst {
                return false;
            }
            lastbyte = lastbyte.wrapping_sub(vm.memory[src]);
            vm.memory[dst + idx] = lastbyte;
            src += 1;
            idx += numchannels;
        }
    }

    filter.filteredblockaddress = length as u32;
    filter.filteredblocklength = length as u32;
    true
}

fn execute_filter_e8(
    filter: &mut RarFilter,
    vm: &mut VirtualMachine,
    pos: usize,
    e9also: bool,
) -> bool {
    let length = filter.initialregisters[4] as usize;
    let filesize: u32 = 0x1000000;

    if length as u32 > PROGRAM_WORK_SIZE || length <= 4 {
        return false;
    }

    let mut i = 0usize;
    while i + 5 <= length {
        if vm.memory[i] == 0xE8 || (e9also && vm.memory[i] == 0xE9) {
            let currpos = (pos as u32).wrapping_add(i as u32 + 1);
            let address = vm.read_32(i + 1) as i32;
            if address < 0 && currpos >= !(address as u32).wrapping_add(1) {
                vm.write_32(i + 1, (address as u32).wrapping_add(filesize));
            } else if address >= 0 && (address as u32) < filesize {
                vm.write_32(i + 1, (address as u32).wrapping_sub(currpos));
            }
            i += 4;
        }
        i += 1;
    }

    filter.filteredblockaddress = 0;
    filter.filteredblocklength = length as u32;
    true
}

fn execute_filter_rgb(filter: &mut RarFilter, vm: &mut VirtualMachine) -> bool {
    let stride = filter.initialregisters[0] as usize;
    let byteoffset = filter.initialregisters[1] as usize;
    let blocklength = filter.initialregisters[4] as usize;

    if blocklength as u32 > PROGRAM_WORK_SIZE / 2
        || stride > blocklength
        || blocklength < 3
        || byteoffset > 2
    {
        return false;
    }

    let mut src = 0usize;
    let dst = blocklength;
    for i in 0..3usize {
        let mut byte = 0u8;
        let mut prev = dst as i64 + i as i64 - stride as i64;
        let mut j = i;
        while j < blocklength {
            if src >= dst {
                return false;
            }
            if prev >= dst as i64 {
                let p = prev as usize;
                let delta1 = (vm.memory[p + 3] as i32 - vm.memory[p] as i32).abs();
                let delta2 = (byte as i32 - vm.memory[p] as i32).abs();
                let delta3 = (vm.memory[p + 3] as i32 - vm.memory[p] as i32 + byte as i32
                    - vm.memory[p] as i32)
                    .abs();
                if delta1 > delta2 || delta1 > delta3 {
                    byte = if delta2 <= delta3 {
                        vm.memory[p + 3]
                    } else {
                        vm.memory[p]
                    };
                }
            }
            byte = byte.wrapping_sub(vm.memory[src]);
            vm.memory[dst + j] = byte;
            src += 1;
            prev += 3;
            j += 3;
        }
    }
    let mut i = byteoffset;
    while i + 2 < blocklength {
        let t = vm.memory[dst + i];
        vm.memory[dst + i] = t.wrapping_add(vm.memory[dst + i + 1]);
        let t = vm.memory[dst + i + 2];
        vm.memory[dst + i + 2] = t.wrapping_add(vm.memory[dst + i + 1]);
        i += 3;
    }

    filter.filteredblockaddress = blocklength as u32;
    filter.filteredblocklength = blocklength as u32;
    true
}

fn execute_filter_audio(filter: &mut RarFilter, vm: &mut VirtualMachine) -> bool {
    let length = filter.initialregisters[4] as usize;
    let numchannels = filter.initialregisters[0] as usize;

    if length as u32 > PROGRAM_WORK_SIZE / 2 || numchannels == 0 || numchannels > 128 {
        return false;
    }

    let mut src = 0usize;
    let dst = length;
    for i in 0..numchannels {
        let mut state = AudioState::default();
        let mut j = i;
        while j < length {
            if src >= dst {
                return false;
            }
            let delta = vm.memory[src] as i8;
            src += 1;
            state.delta[2] = state.delta[1];
            state.delta[1] = state.lastdelta.wrapping_sub(state.delta[0]);
            state.delta[0] = state.lastdelta;
            let predbyte = ((8 * state.lastbyte as i32
                + state.weight[0] as i32 * state.delta[0] as i32
                + state.weight[1] as i32 * state.delta[1] as i32
                + state.weight[2] as i32 * state.delta[2] as i32)
                >> 3) as u8;
            let byte = predbyte.wrapping_sub(delta as u8);
            let prederror = delta as i32 * 8;
            state.error[0] += prederror.abs();
            state.error[1] += (prederror - state.delta[0] as i32).abs();
            state.error[2] += (prederror + state.delta[0] as i32).abs();
            state.error[3] += (prederror - state.delta[1] as i32).abs();
            state.error[4] += (prederror + state.delta[1] as i32).abs();
            state.error[5] += (prederror - state.delta[2] as i32).abs();
            state.error[6] += (prederror + state.delta[2] as i32).abs();
            state.lastdelta = byte.wrapping_sub(state.lastbyte) as i8;
            vm.memory[dst + j] = byte;
            state.lastbyte = byte;
            // C: `!(state.count++ & 0x1F)` — tests the count before increment.
            let fire = state.count & 0x1F == 0;
            state.count = state.count.wrapping_add(1);
            if fire {
                let mut idx = 0usize;
                for k in 1..7usize {
                    if state.error[k] < state.error[idx] {
                        idx = k;
                    }
                }
                state.error = [0; 11];
                match idx {
                    1 => {
                        if state.weight[0] >= -16 {
                            state.weight[0] -= 1;
                        }
                    }
                    2 => {
                        if state.weight[0] < 16 {
                            state.weight[0] += 1;
                        }
                    }
                    3 => {
                        if state.weight[1] >= -16 {
                            state.weight[1] -= 1;
                        }
                    }
                    4 => {
                        if state.weight[1] < 16 {
                            state.weight[1] += 1;
                        }
                    }
                    5 => {
                        if state.weight[2] >= -16 {
                            state.weight[2] -= 1;
                        }
                    }
                    6 => {
                        if state.weight[2] < 16 {
                            state.weight[2] += 1;
                        }
                    }
                    _ => {}
                }
            }
            j += numchannels;
        }
    }

    filter.filteredblockaddress = length as u32;
    filter.filteredblocklength = length as u32;
    true
}

/// `execute_filter`: dispatch on the program fingerprint.
pub(crate) fn execute_filter(
    filter: &mut RarFilter,
    prog_fingerprint: u64,
    vm: &mut VirtualMachine,
    pos: usize,
) -> Result<(), Error> {
    let ok = match prog_fingerprint {
        0x001D_0E06_077D => execute_filter_delta(filter, vm),
        0x0035_AD57_6887 => execute_filter_e8(filter, vm, pos, false),
        0x0039_3CD7_E57E => execute_filter_e8(filter, vm, pos, true),
        0x0095_1C2C_5DC8 => execute_filter_rgb(filter, vm),
        0x00D8_BC85_E701 => execute_filter_audio(filter, vm),
        _ => return Err(Error::Unsupported("no support for RAR VM program filter")),
    };
    if ok {
        Ok(())
    } else {
        Err(Error::InvalidData("filter execution failed"))
    }
}
