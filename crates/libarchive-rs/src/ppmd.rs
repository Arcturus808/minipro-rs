//! PPMd variant-H decoder.
//!
//! Port of the decode path of `archive_ppmd7.c` (public domain, Igor Pavlov /
//! Dmitry Shkarin) as bundled in libarchive. The C code manages a `Base`
//! memory arena and addresses everything by `UInt32` offsets; this port keeps
//! that model — all "pointers" are byte offsets into [`Ppmd7::base`].

const PPMD_NUM_INDEXES: usize = 38; // N1+N2+N3+N4 = 4+4+4+26
const PPMD7_MAX_ORDER: usize = 64;
const PPMD7_MIN_MEM_SIZE: u32 = 1 << 11;
const PPMD7_MAX_MEM_SIZE: u32 = 0xFFFF_FFFF - 12 * 3;

const PPMD_INT_BITS: u32 = 7;
const PPMD_PERIOD_BITS: u32 = 7;
const PPMD_BIN_SCALE: u32 = 1 << (PPMD_INT_BITS + PPMD_PERIOD_BITS);

const K_TOP_VALUE: u32 = 1 << 24;
const MAX_FREQ: u8 = 124;
const UNIT_SIZE: u32 = 12;

const K_INIT_BIN_ESC: [u16; 8] = [
    0x3CDD, 0x1F3F, 0x59BF, 0x48F3, 0x64A1, 0x5ABC, 0x6632, 0x6051,
];
const PPMD7_K_EXP_ESCAPE: [u8; 16] = [25, 14, 9, 7, 5, 5, 4, 4, 4, 3, 3, 3, 2, 2, 2, 2];

#[inline]
fn update_prob_0(prob: u16) -> u16 {
    prob + (1 << PPMD_INT_BITS) - ((prob + (1 << (PPMD_PERIOD_BITS - 2))) >> PPMD_PERIOD_BITS)
}
#[inline]
fn update_prob_1(prob: u16) -> u16 {
    prob - ((prob + (1 << (PPMD_PERIOD_BITS - 2))) >> PPMD_PERIOD_BITS)
}
#[inline]
fn see_update(shift: &mut u8, summ: &mut u16, count: &mut u8) {
    if *shift < PPMD_PERIOD_BITS as u8 && {
        *count -= 1;
        *count == 0
    } {
        *summ <<= 1;
        *count = 3 << *shift;
        *shift += 1;
    }
}

/// RAR range decoder (`CPpmd7z_RangeDec` with the RAR vtable).
#[derive(Debug, Default)]
pub(crate) struct RangeDec {
    pub range: u32,
    pub code: u32,
    pub low: u32,
    pub bottom: u32,
}

impl RangeDec {
    /// `PpmdRAR_RangeDec_Init`.
    pub fn init_rar(&mut self, read: &mut dyn FnMut() -> u8) -> bool {
        self.low = 0;
        self.bottom = 0;
        self.range = 0xFFFF_FFFF;
        self.code = 0;
        for _ in 0..4 {
            self.code = (self.code << 8) | read() as u32;
        }
        if self.code == 0xFFFF_FFFF {
            return false;
        }
        self.bottom = 0x8000;
        true
    }

    /// `Range_GetThreshold`.
    #[inline]
    fn get_threshold(&mut self, total: u32) -> u32 {
        self.range /= total;
        self.code.wrapping_sub(self.low) / self.range
    }

    /// `Range_Normalize`.
    fn normalize(&mut self, read: &mut dyn FnMut() -> u8) {
        loop {
            if (self.low ^ self.low.wrapping_add(self.range)) >= K_TOP_VALUE {
                if self.range >= self.bottom {
                    break;
                }
                self.range = self.low.wrapping_neg() & (self.bottom - 1);
            }
            self.code = (self.code << 8) | read() as u32;
            self.range <<= 8;
            self.low <<= 8;
        }
    }

    /// `Range_Decode_RAR`.
    #[inline]
    fn decode(&mut self, start: u32, size: u32, read: &mut dyn FnMut() -> u8) {
        self.low = self.low.wrapping_add(start.wrapping_mul(self.range));
        self.range = self.range.wrapping_mul(size);
        self.normalize(read);
    }

    /// `Range_DecodeBit_RAR`.
    #[inline]
    fn decode_bit(&mut self, size0: u32, read: &mut dyn FnMut() -> u8) -> u32 {
        let value = self.get_threshold(PPMD_BIN_SCALE);
        if value < size0 {
            self.decode(0, size0, read);
            0
        } else {
            self.decode(size0, PPMD_BIN_SCALE - size0, read);
            1
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct See {
    summ: u16,
    shift: u8,
    count: u8,
}

/// `CPpmd7` — the PPMd model context plus its memory arena.
pub(crate) struct Ppmd7 {
    // Decoding state
    min_context: u32,
    max_context: u32,
    found_state: u32,
    order_fall: u32,
    init_esc: u32,
    prev_success: u32,
    max_order: u32,
    hi_bits_flag: u32,
    run_length: i32,
    init_rl: i32,

    // Arena
    size: u32,
    glue_count: u32,
    base: Vec<u8>,
    lo_unit: u32,
    hi_unit: u32,
    text: u32,
    units_start: u32,
    align_offset: u32,

    indx2units: [u8; PPMD_NUM_INDEXES],
    units2indx: [u8; 128],
    free_list: [u32; PPMD_NUM_INDEXES],
    ns2indx: [u8; 256],
    ns2bsindx: [u8; 256],
    hb2flag: [u8; 256],
    dummy_see: See,
    see: [[See; 16]; 25],
    bin_summ: [[u16; 64]; 128],
}

// ---- little-endian accessors into the arena ----
impl Ppmd7 {
    #[inline]
    fn r8(&self, off: u32) -> u8 {
        self.base[off as usize]
    }
    #[inline]
    fn w8(&mut self, off: u32, v: u8) {
        self.base[off as usize] = v;
    }
    #[inline]
    fn r16(&self, off: u32) -> u16 {
        let o = off as usize;
        u16::from_le_bytes([self.base[o], self.base[o + 1]])
    }
    #[inline]
    fn w16(&mut self, off: u32, v: u16) {
        self.base[off as usize..off as usize + 2].copy_from_slice(&v.to_le_bytes());
    }
    #[inline]
    fn r32(&self, off: u32) -> u32 {
        let o = off as usize;
        u32::from_le_bytes([
            self.base[o],
            self.base[o + 1],
            self.base[o + 2],
            self.base[o + 3],
        ])
    }
    #[inline]
    fn w32(&mut self, off: u32, v: u32) {
        self.base[off as usize..off as usize + 4].copy_from_slice(&v.to_le_bytes());
    }

    // CPpmd7_Context { NumStats@0, SummFreq@2, Stats@4, Suffix@8 }
    #[inline]
    fn ctx_numstats(&self, c: u32) -> u16 {
        self.r16(c)
    }
    #[inline]
    fn set_ctx_numstats(&mut self, c: u32, v: u16) {
        self.w16(c, v);
    }
    #[inline]
    fn ctx_summfreq(&self, c: u32) -> u16 {
        self.r16(c + 2)
    }
    #[inline]
    fn set_ctx_summfreq(&mut self, c: u32, v: u16) {
        self.w16(c + 2, v);
    }
    #[inline]
    fn ctx_stats(&self, c: u32) -> u32 {
        self.r32(c + 4)
    }
    #[inline]
    fn set_ctx_stats(&mut self, c: u32, v: u32) {
        self.w32(c + 4, v);
    }
    #[inline]
    fn ctx_suffix(&self, c: u32) -> u32 {
        self.r32(c + 8)
    }
    #[inline]
    fn set_ctx_suffix(&mut self, c: u32, v: u32) {
        self.w32(c + 8, v);
    }
    /// `Ppmd7Context_OneState` — state struct overlaid at `SummFreq`.
    #[inline]
    fn ctx_one_state(&self, c: u32) -> u32 {
        c + 2
    }

    // CPpmd_State { Symbol@0, Freq@1, SuccessorLow@2, SuccessorHigh@4 }
    #[inline]
    fn st_symbol(&self, s: u32) -> u8 {
        self.r8(s)
    }
    #[inline]
    fn set_st_symbol(&mut self, s: u32, v: u8) {
        self.w8(s, v);
    }
    #[inline]
    fn st_freq(&self, s: u32) -> u8 {
        self.r8(s + 1)
    }
    #[inline]
    fn set_st_freq(&mut self, s: u32, v: u8) {
        self.w8(s + 1, v);
    }
    #[inline]
    fn st_successor(&self, s: u32) -> u32 {
        self.r16(s + 2) as u32 | ((self.r16(s + 4) as u32) << 16)
    }
    #[inline]
    fn set_st_successor(&mut self, s: u32, v: u32) {
        self.w16(s + 2, (v & 0xFFFF) as u16);
        self.w16(s + 4, (v >> 16) as u16);
    }
    #[inline]
    fn st_read(&self, s: u32) -> (u8, u8, u32) {
        (self.st_symbol(s), self.st_freq(s), self.st_successor(s))
    }
    #[inline]
    fn st_write(&mut self, s: u32, sym: u8, freq: u8, succ: u32) {
        self.set_st_symbol(s, sym);
        self.set_st_freq(s, freq);
        self.set_st_successor(s, succ);
    }

    // CPpmd7_Node { Stamp@0, NU@2, Next@4, Prev@8 }; freelist link @0
    #[inline]
    fn node_stamp(&self, n: u32) -> u16 {
        self.r16(n)
    }
    #[inline]
    fn set_node_stamp(&mut self, n: u32, v: u16) {
        self.w16(n, v);
    }
    #[inline]
    fn node_nu(&self, n: u32) -> u16 {
        self.r16(n + 2)
    }
    #[inline]
    fn set_node_nu(&mut self, n: u32, v: u16) {
        self.w16(n + 2, v);
    }
    #[inline]
    fn node_next(&self, n: u32) -> u32 {
        self.r32(n + 4)
    }
    #[inline]
    fn set_node_next(&mut self, n: u32, v: u32) {
        self.w32(n + 4, v);
    }
    #[inline]
    fn node_prev(&self, n: u32) -> u32 {
        self.r32(n + 8)
    }
    #[inline]
    fn set_node_prev(&mut self, n: u32, v: u32) {
        self.w32(n + 8, v);
    }

    #[inline]
    fn i2u(&self, indx: usize) -> u32 {
        self.indx2units[indx] as u32
    }
    #[inline]
    fn u2i(&self, nu: u32) -> usize {
        self.units2indx[nu as usize - 1] as usize
    }
    #[inline]
    fn u2b(nu: u32) -> u32 {
        nu * UNIT_SIZE
    }
}

impl Default for Ppmd7 {
    fn default() -> Self {
        Self {
            min_context: 0,
            max_context: 0,
            found_state: 0,
            order_fall: 0,
            init_esc: 0,
            prev_success: 0,
            max_order: 0,
            hi_bits_flag: 0,
            run_length: 0,
            init_rl: 0,
            size: 0,
            glue_count: 0,
            base: Vec::new(),
            lo_unit: 0,
            hi_unit: 0,
            text: 0,
            units_start: 0,
            align_offset: 0,
            indx2units: [0; PPMD_NUM_INDEXES],
            units2indx: [0; 128],
            free_list: [0; PPMD_NUM_INDEXES],
            ns2indx: [0; 256],
            ns2bsindx: [0; 256],
            hb2flag: [0; 256],
            dummy_see: See::default(),
            see: [[See::default(); 16]; 25],
            bin_summ: [[0; 64]; 128],
        }
    }
}

impl Ppmd7 {
    pub fn set_init_esc(&mut self, v: u32) {
        self.init_esc = v;
    }

    /// `Ppmd7_Construct`.
    pub fn construct(&mut self) {
        self.base = Vec::new();
        self.size = 0;

        let mut k = 0usize;
        for i in 0..PPMD_NUM_INDEXES {
            let mut step = if i >= 12 { 4 } else { (i >> 2) + 1 };
            loop {
                self.units2indx[k] = i as u8;
                k += 1;
                step -= 1;
                if step == 0 {
                    break;
                }
            }
            self.indx2units[i] = k as u8;
        }

        self.ns2bsindx[0] = 0 << 1;
        self.ns2bsindx[1] = 1 << 1;
        for v in self.ns2bsindx[2..11].iter_mut() {
            *v = 2 << 1;
        }
        for v in self.ns2bsindx[11..].iter_mut() {
            *v = 3 << 1;
        }

        for i in 0..3 {
            self.ns2indx[i] = i as u8;
        }
        let mut m = 3u8;
        let mut kk = 1u32;
        for i in 3..256 {
            self.ns2indx[i] = m;
            kk -= 1;
            if kk == 0 {
                m += 1;
                kk = m as u32 - 2;
            }
        }

        for v in self.hb2flag[0..0x40].iter_mut() {
            *v = 0;
        }
        for v in self.hb2flag[0x40..0x100].iter_mut() {
            *v = 8;
        }
    }

    /// `Ppmd7_Free`.
    pub fn free(&mut self) {
        self.base = Vec::new();
        self.size = 0;
    }

    /// `Ppmd7_Alloc`.
    pub fn alloc(&mut self, size: u32) -> bool {
        if !(PPMD7_MIN_MEM_SIZE..=PPMD7_MAX_MEM_SIZE).contains(&size) {
            // The C version only checks size >= UNIT_SIZE, but keep the
            // documented bounds for sanity.
        }
        if self.base.is_empty() || self.size != size {
            if size < UNIT_SIZE {
                return false;
            }
            self.free();
            self.align_offset = 4 - (size & 3);
            self.base = vec![0; (self.align_offset + size + UNIT_SIZE) as usize];
            self.size = size;
        }
        true
    }

    fn insert_node(&mut self, node: u32, indx: usize) {
        let link = self.free_list[indx];
        self.w32(node, link);
        self.free_list[indx] = node;
    }

    fn remove_node(&mut self, indx: usize) -> u32 {
        let node = self.free_list[indx];
        let next = self.r32(node);
        self.free_list[indx] = next;
        node
    }

    fn split_block(&mut self, ptr: u32, old_indx: usize, new_indx: usize) {
        let nu = self.i2u(old_indx) - self.i2u(new_indx);
        let ptr = ptr + Self::u2b(self.i2u(new_indx));
        let mut i = self.u2i(nu);
        if self.i2u(i) != nu {
            i -= 1;
            let k = self.i2u(i);
            self.insert_node(ptr + Self::u2b(k), (nu - k - 1) as usize);
        }
        self.insert_node(ptr, i);
    }

    /// `GlueFreeBlocks`.
    fn glue_free_blocks(&mut self) {
        let head = self.align_offset + self.size;
        let mut n = head;
        self.glue_count = 255;

        // Create doubly-linked list of free blocks.
        for i in 0..PPMD_NUM_INDEXES {
            let nu = self.i2u(i);
            let mut next = self.free_list[i];
            self.free_list[i] = 0;
            while next != 0 {
                let node = next;
                self.set_node_next(node, n);
                self.set_node_prev(n, next);
                n = next;
                next = self.r32(node); // freelist link at offset 0
                self.set_node_stamp(node, 0);
                self.set_node_nu(node, nu as u16);
            }
        }
        self.set_node_stamp(head, 1);
        self.set_node_next(head, n);
        self.set_node_prev(n, head);
        if self.lo_unit != self.hi_unit {
            self.set_node_stamp(self.lo_unit, 1);
        }

        // Glue free blocks.
        while n != head {
            let node = n;
            let mut nu = self.node_nu(node) as u32;
            loop {
                let node2 = n + nu * UNIT_SIZE;
                nu += self.node_nu(node2) as u32;
                if self.node_stamp(node2) != 0 || nu >= 0x10000 {
                    break;
                }
                let (p2, n2) = (self.node_prev(node2), self.node_next(node2));
                self.set_node_next(p2, n2);
                self.set_node_prev(n2, p2);
                self.set_node_nu(node, nu as u16);
            }
            n = self.node_next(node);
        }

        // Fill lists of free blocks.
        n = self.node_next(head);
        while n != head {
            let mut node = n;
            let next = self.node_next(node);
            let mut nu = self.node_nu(node) as u32;
            while nu > 128 {
                self.insert_node(node, PPMD_NUM_INDEXES - 1);
                nu -= 128;
                node += 128 * UNIT_SIZE;
            }
            let mut i = self.u2i(nu);
            if self.i2u(i) != nu {
                i -= 1;
                let k = self.i2u(i);
                self.insert_node(node + k * UNIT_SIZE, (nu - k - 1) as usize);
            }
            self.insert_node(node, i);
            n = next;
        }
    }

    fn alloc_units_rare(&mut self, indx: usize) -> u32 {
        if self.glue_count == 0 {
            self.glue_free_blocks();
            if self.free_list[indx] != 0 {
                return self.remove_node(indx);
            }
        }
        let mut i = indx;
        loop {
            i += 1;
            if i == PPMD_NUM_INDEXES {
                let num_bytes = Self::u2b(self.i2u(indx));
                self.glue_count -= 1;
                return if self.units_start - self.text > num_bytes {
                    self.units_start -= num_bytes;
                    self.units_start
                } else {
                    0
                };
            }
            if self.free_list[i] != 0 {
                break;
            }
        }
        let ret_val = self.remove_node(i);
        self.split_block(ret_val, i, indx);
        ret_val
    }

    fn alloc_units(&mut self, indx: usize) -> u32 {
        if self.free_list[indx] != 0 {
            return self.remove_node(indx);
        }
        let num_bytes = Self::u2b(self.i2u(indx));
        if num_bytes <= self.hi_unit - self.lo_unit {
            let ret = self.lo_unit;
            self.lo_unit += num_bytes;
            return ret;
        }
        self.alloc_units_rare(indx)
    }

    /// `MyMem12Cpy`: copy `num*12` bytes within the arena.
    fn mem12cpy(&mut self, dest: u32, src: u32, num: u32) {
        let n = (num * UNIT_SIZE) as usize;
        self.base
            .copy_within(src as usize..src as usize + n, dest as usize);
    }

    fn shrink_units(&mut self, old_ptr: u32, old_nu: u32, new_nu: u32) -> u32 {
        let i0 = self.u2i(old_nu);
        let i1 = self.u2i(new_nu);
        if i0 == i1 {
            return old_ptr;
        }
        if self.free_list[i1] != 0 {
            let ptr = self.remove_node(i1);
            self.mem12cpy(ptr, old_ptr, new_nu);
            self.insert_node(old_ptr, i0);
            return ptr;
        }
        self.split_block(old_ptr, i0, i1);
        old_ptr
    }

    /// `RestartModel`.
    fn restart_model(&mut self) {
        self.free_list = [0; PPMD_NUM_INDEXES];
        self.text = self.align_offset;
        self.hi_unit = self.text + self.size;
        self.units_start = self.hi_unit - self.size / 8 / UNIT_SIZE * 7 * UNIT_SIZE;
        self.lo_unit = self.units_start;
        self.glue_count = 0;

        self.order_fall = self.max_order;
        self.init_rl = -((if self.max_order < 12 {
            self.max_order
        } else {
            12
        }) as i32)
            - 1;
        self.run_length = self.init_rl;
        self.prev_success = 0;

        self.hi_unit -= UNIT_SIZE;
        self.min_context = self.hi_unit;
        self.max_context = self.hi_unit;
        self.set_ctx_suffix(self.min_context, 0);
        self.set_ctx_numstats(self.min_context, 256);
        self.set_ctx_summfreq(self.min_context, 256 + 1);
        self.found_state = self.lo_unit;
        self.lo_unit += Self::u2b(256 / 2);
        self.set_ctx_stats(self.min_context, self.found_state);
        for i in 0..256u32 {
            let s = self.found_state + i * 6;
            self.st_write(s, i as u8, 1, 0);
        }

        for i in 0..128usize {
            for (k, &esc) in K_INIT_BIN_ESC.iter().enumerate() {
                let val = (PPMD_BIN_SCALE - esc as u32 / (i as u32 + 2)) as u16;
                for m in (k..64).step_by(8) {
                    self.bin_summ[i][m] = val;
                }
            }
        }

        for i in 0..25usize {
            for k in 0..16usize {
                let s = &mut self.see[i][k];
                s.shift = (PPMD_PERIOD_BITS - 4) as u8;
                s.summ = ((5 * i + 10) << s.shift) as u16;
                s.count = 4;
            }
        }
    }

    /// `Ppmd7_Init`.
    pub fn init(&mut self, max_order: u32) {
        self.max_order = max_order;
        self.restart_model();
        self.dummy_see.shift = PPMD_PERIOD_BITS as u8;
        self.dummy_see.summ = 0;
        self.dummy_see.count = 64;
    }

    /// `CreateSuccessors`: returns a context ref, 0 on allocation failure.
    fn create_successors(&mut self, skip: bool) -> u32 {
        let mut up_state = (0u8, 0u8, 0u32);
        let mut c = self.min_context;
        let up_branch = self.st_successor(self.found_state);
        let mut ps = [0u32; PPMD7_MAX_ORDER];
        let mut num_ps = 0usize;

        if !skip {
            ps[num_ps] = self.found_state;
            num_ps += 1;
        }

        while self.ctx_suffix(c) != 0 {
            let c_next = self.ctx_suffix(c);
            c = c_next;
            let s = if self.ctx_numstats(c) != 1 {
                let mut s = self.ctx_stats(c);
                let target = self.st_symbol(self.found_state);
                while self.st_symbol(s) != target {
                    s += 6;
                }
                s
            } else {
                self.ctx_one_state(c)
            };
            let successor = self.st_successor(s);
            if successor != up_branch {
                c = successor;
                if num_ps == 0 {
                    return c;
                }
                break;
            }
            ps[num_ps] = s;
            num_ps += 1;
        }

        up_state.0 = self.r8(up_branch);
        up_state.2 = up_branch + 1;

        if self.ctx_numstats(c) == 1 {
            up_state.1 = self.st_freq(self.ctx_one_state(c));
        } else {
            let mut s = self.ctx_stats(c);
            while self.st_symbol(s) != up_state.0 {
                s += 6;
            }
            let cf = self.st_freq(s) as u32 - 1;
            let s0 = self.ctx_summfreq(c) as u32 - self.ctx_numstats(c) as u32 - cf;
            up_state.1 = (1 + if 2 * cf <= s0 {
                (5 * cf > s0) as u32
            } else {
                (2 * cf + 3 * s0 - 1) / (2 * s0)
            }) as u8;
        }

        while num_ps != 0 {
            // Create child context.
            let c1 = if self.hi_unit != self.lo_unit {
                self.hi_unit -= UNIT_SIZE;
                self.hi_unit
            } else if self.free_list[0] != 0 {
                self.remove_node(0)
            } else {
                let a = self.alloc_units_rare(0);
                if a == 0 {
                    return 0;
                }
                a
            };
            self.set_ctx_numstats(c1, 1);
            self.st_write(self.ctx_one_state(c1), up_state.0, up_state.1, up_state.2);
            self.set_ctx_suffix(c1, c);
            num_ps -= 1;
            self.set_st_successor(ps[num_ps], c1);
            c = c1;
        }
        c
    }

    /// `UpdateModel`.
    fn update_model(&mut self) {
        let mut f_successor = self.st_successor(self.found_state);
        let mut successor;

        if self.st_freq(self.found_state) < MAX_FREQ / 4 && self.ctx_suffix(self.min_context) != 0 {
            let c = self.ctx_suffix(self.min_context);
            if self.ctx_numstats(c) == 1 {
                let s = self.ctx_one_state(c);
                if self.st_freq(s) < 32 {
                    self.set_st_freq(s, self.st_freq(s) + 1);
                }
            } else {
                let mut s = self.ctx_stats(c);
                let target = self.st_symbol(self.found_state);
                if self.st_symbol(s) != target {
                    loop {
                        s += 6;
                        if self.st_symbol(s) == target {
                            break;
                        }
                    }
                    if self.st_freq(s) >= self.st_freq(s - 6) {
                        // SwapStates(&s[0], &s[-1])
                        let (a0, a1, a2) = self.st_read(s);
                        let (b0, b1, b2) = self.st_read(s - 6);
                        self.st_write(s, b0, b1, b2);
                        self.st_write(s - 6, a0, a1, a2);
                        s -= 6;
                    }
                }
                if self.st_freq(s) < MAX_FREQ - 9 {
                    self.set_st_freq(s, self.st_freq(s) + 2);
                    self.set_ctx_summfreq(c, self.ctx_summfreq(c) + 2);
                }
            }
        }

        if self.order_fall == 0 {
            let cs = self.create_successors(true);
            self.min_context = cs;
            self.max_context = cs;
            if self.min_context == 0 {
                self.restart_model();
                return;
            }
            let mc = self.min_context;
            self.set_st_successor(self.found_state, mc);
            return;
        }

        let sym = self.st_symbol(self.found_state);
        self.w8(self.text, sym);
        self.text += 1;
        successor = self.text;
        if self.text >= self.units_start {
            self.restart_model();
            return;
        }

        if f_successor != 0 {
            if f_successor <= successor {
                let cs = self.create_successors(false);
                if cs == 0 {
                    self.restart_model();
                    return;
                }
                f_successor = cs;
            }
            self.order_fall -= 1;
            if self.order_fall == 0 {
                successor = f_successor;
                if self.max_context != self.min_context {
                    self.text -= 1;
                }
            }
        } else {
            self.set_st_successor(self.found_state, successor);
            f_successor = self.min_context;
        }

        let ns = self.ctx_numstats(self.min_context) as u32;
        let s0 = self.ctx_summfreq(self.min_context) as u32
            - ns
            - (self.st_freq(self.found_state) as u32 - 1);

        let mut c = self.max_context;
        while c != self.min_context {
            let ns1 = self.ctx_numstats(c) as u32;
            if ns1 != 1 {
                if ns1 & 1 == 0 {
                    // Expand for one UNIT.
                    let old_nu = ns1 >> 1;
                    let i = self.u2i(old_nu);
                    if i != self.u2i(old_nu + 1) {
                        let ptr = self.alloc_units(i + 1);
                        if ptr == 0 {
                            self.restart_model();
                            return;
                        }
                        let old_ptr = self.ctx_stats(c);
                        self.mem12cpy(ptr, old_ptr, old_nu);
                        self.insert_node(old_ptr, i);
                        self.set_ctx_stats(c, ptr);
                    }
                }
                let add = ((2 * ns1 < ns) as u32)
                    + 2 * ((4 * ns1 <= ns) as u32)
                        * ((self.ctx_summfreq(c) as u32) <= 8 * ns1) as u32;
                self.set_ctx_summfreq(c, (self.ctx_summfreq(c) as u32 + add) as u16);
            } else {
                let s = self.alloc_units(0);
                if s == 0 {
                    self.restart_model();
                    return;
                }
                let (sym0, freq0, succ0) = self.st_read(self.ctx_one_state(c));
                self.st_write(s, sym0, freq0, succ0);
                self.set_ctx_stats(c, s);
                let f = if self.st_freq(s) < MAX_FREQ / 4 - 1 {
                    self.st_freq(s) << 1
                } else {
                    MAX_FREQ - 4
                };
                self.set_st_freq(s, f);
                let sf = f as u32 + self.init_esc + (ns > 3) as u32;
                self.set_ctx_summfreq(c, sf as u16);
            }
            let cf = 2 * self.st_freq(self.found_state) as u32 * (self.ctx_summfreq(c) as u32 + 6);
            let sf = s0 + self.ctx_summfreq(c) as u32;
            let cfv;
            if cf < 6 * sf {
                cfv = 1 + (cf > sf) as u32 + (cf >= 4 * sf) as u32;
                self.set_ctx_summfreq(c, self.ctx_summfreq(c) + 3);
            } else {
                cfv = 4 + (cf >= 9 * sf) as u32 + (cf >= 12 * sf) as u32 + (cf >= 15 * sf) as u32;
                self.set_ctx_summfreq(c, self.ctx_summfreq(c) + cfv as u16);
            }
            let s = self.ctx_stats(c) + ns1 * 6;
            self.set_st_successor(s, successor);
            self.set_st_symbol(s, self.st_symbol(self.found_state));
            self.set_st_freq(s, cfv as u8);
            self.set_ctx_numstats(c, (ns1 + 1) as u16);
            c = self.ctx_suffix(c);
        }
        self.max_context = f_successor;
        self.min_context = f_successor;
    }

    /// `Rescale`.
    fn rescale(&mut self) {
        let stats = self.ctx_stats(self.min_context);
        let mut s = self.found_state;
        // Move found state to the head.
        {
            let (a0, a1, a2) = self.st_read(s);
            while s != stats {
                let (b0, b1, b2) = self.st_read(s - 6);
                self.st_write(s, b0, b1, b2);
                s -= 6;
            }
            self.st_write(s, a0, a1, a2);
        }
        let mut esc_freq = self.ctx_summfreq(self.min_context) as u32 - self.st_freq(s) as u32;
        let adder = (self.order_fall != 0) as u32;
        let f = (self.st_freq(s) as u32 + 4 + adder) >> 1;
        self.set_st_freq(s, f as u8);
        let mut sum_freq = f;

        let mut i = self.ctx_numstats(self.min_context) as u32 - 1;
        loop {
            s += 6;
            esc_freq -= self.st_freq(s) as u32;
            let f = (self.st_freq(s) as u32 + adder) >> 1;
            self.set_st_freq(s, f as u8);
            sum_freq += f;
            if self.st_freq(s) > self.st_freq(s - 6) {
                let mut s1 = s;
                let (t0, t1, t2) = self.st_read(s1);
                loop {
                    let (b0, b1, b2) = self.st_read(s1 - 6);
                    self.st_write(s1, b0, b1, b2);
                    s1 -= 6;
                    if !(s1 != stats && t1 > self.st_freq(s1 - 6)) {
                        break;
                    }
                }
                self.st_write(s1, t0, t1, t2);
            }
            i -= 1;
            if i == 0 {
                break;
            }
        }

        if self.st_freq(s) == 0 {
            let num_stats = self.ctx_numstats(self.min_context) as u32;
            let mut i2 = 0u32;
            loop {
                i2 += 1;
                s -= 6;
                if self.st_freq(s) != 0 {
                    break;
                }
            }
            let i = i2;
            esc_freq += i;
            self.set_ctx_numstats(
                self.min_context,
                (self.ctx_numstats(self.min_context) as u32 - i) as u16,
            );
            if self.ctx_numstats(self.min_context) == 1 {
                let (t0, t1, t2) = self.st_read(stats);
                let mut tmp_freq = t1;
                loop {
                    tmp_freq -= tmp_freq >> 1;
                    esc_freq >>= 1;
                    if esc_freq <= 1 {
                        break;
                    }
                }
                self.insert_node(stats, self.u2i((num_stats + 1) >> 1));
                self.found_state = self.ctx_one_state(self.min_context);
                self.st_write(self.found_state, t0, tmp_freq, t2);
                return;
            }
            let n0 = (num_stats + 1) >> 1;
            let n1 = (self.ctx_numstats(self.min_context) as u32 + 1) >> 1;
            if n0 != n1 {
                let stats2 = self.ctx_stats(self.min_context);
                let shrunk = self.shrink_units(stats2, n0, n1);
                self.set_ctx_stats(self.min_context, shrunk);
            }
        }
        let sf = self.ctx_summfreq(self.min_context) as u32;
        let _ = sf;
        self.set_ctx_summfreq(
            self.min_context,
            (sum_freq + esc_freq - (esc_freq >> 1)) as u16,
        );
        self.found_state = self.ctx_stats(self.min_context);
    }

    /// `Ppmd7_MakeEscFreq`: returns see index + esc freq.
    fn make_esc_freq(&mut self, num_masked: u32) -> (u32, u32) {
        let non_masked = self.ctx_numstats(self.min_context) as u32 - num_masked;
        let esc_freq;
        let see_idx;
        if self.ctx_numstats(self.min_context) != 256 {
            let suffix = self.ctx_suffix(self.min_context);
            let suffix_ns = self.ctx_numstats(suffix) as u32;
            let this_ns = self.ctx_numstats(self.min_context) as u32;
            let this_sf = self.ctx_summfreq(self.min_context) as u32;
            let row = self.ns2indx[(non_masked - 1) as usize] as usize;
            let col = (non_masked < suffix_ns - this_ns) as usize
                + 2 * (this_sf < 11 * this_ns) as usize
                + 4 * (num_masked > non_masked) as usize
                + self.hi_bits_flag as usize;
            see_idx = (row * 16 + col) as u32;
            let s = &mut self.see[row][col];
            let r = (s.summ >> s.shift) as u32;
            s.summ -= r as u16;
            esc_freq = r + (r == 0) as u32;
        } else {
            see_idx = u32::MAX; // dummy
            esc_freq = 1;
        }
        (see_idx, esc_freq)
    }

    /// `NextContext`.
    fn next_context(&mut self) {
        let c = self.st_successor(self.found_state);
        if self.order_fall == 0 && c > self.text {
            self.min_context = c;
            self.max_context = c;
        } else {
            self.update_model();
        }
    }

    /// `Ppmd7_Update1`.
    fn update1(&mut self) {
        let s = self.found_state;
        self.set_st_freq(s, self.st_freq(s) + 4);
        self.set_ctx_summfreq(self.min_context, self.ctx_summfreq(self.min_context) + 4);
        if self.st_freq(s) > self.st_freq(s - 6) {
            let (a0, a1, a2) = self.st_read(s);
            let (b0, b1, b2) = self.st_read(s - 6);
            self.st_write(s, b0, b1, b2);
            self.st_write(s - 6, a0, a1, a2);
            self.found_state = s - 6;
            if self.st_freq(self.found_state) > MAX_FREQ {
                self.rescale();
            }
        }
        self.next_context();
    }

    /// `Ppmd7_Update1_0`.
    fn update1_0(&mut self) {
        self.prev_success = (2 * self.st_freq(self.found_state) as u32
            > self.ctx_summfreq(self.min_context) as u32) as u32;
        self.run_length += self.prev_success as i32;
        self.set_ctx_summfreq(self.min_context, self.ctx_summfreq(self.min_context) + 4);
        let f = self.st_freq(self.found_state) + 4;
        self.set_st_freq(self.found_state, f);
        if f > MAX_FREQ {
            self.rescale();
        }
        self.next_context();
    }

    /// `Ppmd7_UpdateBin`.
    fn update_bin(&mut self) {
        let s = self.found_state;
        let f = self.st_freq(s);
        self.set_st_freq(s, f + (f < 128) as u8);
        self.prev_success = 1;
        self.run_length += 1;
        self.next_context();
    }

    /// `Ppmd7_Update2`.
    fn update2(&mut self) {
        self.set_ctx_summfreq(self.min_context, self.ctx_summfreq(self.min_context) + 4);
        let f = self.st_freq(self.found_state) + 4;
        self.set_st_freq(self.found_state, f);
        if f > MAX_FREQ {
            self.rescale();
        }
        self.run_length = self.init_rl;
        self.update_model();
    }

    /// `Ppmd7_GetBinSumm` — flattened index into `bin_summ`.
    fn get_bin_summ_idx(&self) -> (usize, usize) {
        let one = self.ctx_one_state(self.min_context);
        let freq = self.st_freq(one) as usize;
        let suffix = self.ctx_suffix(self.min_context);
        let suffix_ns = self.ctx_numstats(suffix) as usize;
        let hb = self.hb2flag[self.st_symbol(self.found_state) as usize] as usize;
        let hb_one = self.hb2flag[self.st_symbol(one) as usize] as usize;
        (
            freq - 1,
            self.prev_success as usize
                + self.ns2bsindx[suffix_ns - 1] as usize
                + hb
                + 2 * hb_one
                + ((self.run_length >> 26) & 0x20) as usize,
        )
    }

    /// `Ppmd7_DecodeSymbol`. Returns the decoded symbol (0..=255),
    /// -1 when the context ran out (end of stream), -2 on bad data.
    pub fn decode_symbol(&mut self, rc: &mut RangeDec, read: &mut dyn FnMut() -> u8) -> i32 {
        // PPMD_SetAllBitsIn256Bytes: -1 = "include in freq sum".
        let mut char_mask = [-1i8; 256];
        if self.ctx_numstats(self.min_context) != 1 {
            let mut s = self.ctx_stats(self.min_context);
            let summ_freq = self.ctx_summfreq(self.min_context) as u32;
            let count = rc.get_threshold(summ_freq);
            let mut hi_cnt = self.st_freq(s) as u32;
            if count < hi_cnt {
                rc.decode(0, self.st_freq(s) as u32, read);
                self.found_state = s;
                let symbol = self.st_symbol(s);
                self.update1_0();
                return symbol as i32;
            }
            self.prev_success = 0;
            let mut i = self.ctx_numstats(self.min_context) as u32 - 1;
            loop {
                s += 6;
                hi_cnt += self.st_freq(s) as u32;
                if hi_cnt > count {
                    rc.decode(
                        hi_cnt - self.st_freq(s) as u32,
                        self.st_freq(s) as u32,
                        read,
                    );
                    self.found_state = s;
                    let symbol = self.st_symbol(s);
                    self.update1();
                    return symbol as i32;
                }
                i -= 1;
                if i == 0 {
                    break;
                }
            }
            if count >= summ_freq {
                return -2;
            }
            self.hi_bits_flag = self.hb2flag[self.st_symbol(self.found_state) as usize] as u32;
            rc.decode(hi_cnt, summ_freq - hi_cnt, read);
            char_mask[self.st_symbol(s) as usize] = 0;
            let mut i = self.ctx_numstats(self.min_context) as u32 - 1;
            loop {
                s -= 6;
                char_mask[self.st_symbol(s) as usize] = 0;
                i -= 1;
                if i == 0 {
                    break;
                }
            }
        } else {
            let (bi, bj) = self.get_bin_summ_idx();
            let mut prob = self.bin_summ[bi][bj];
            if rc.decode_bit(prob as u32, read) == 0 {
                prob = update_prob_0(prob);
                self.bin_summ[bi][bj] = prob;
                let s = self.ctx_one_state(self.min_context);
                self.found_state = s;
                let symbol = self.st_symbol(s);
                self.update_bin();
                return symbol as i32;
            }
            prob = update_prob_1(prob);
            self.bin_summ[bi][bj] = prob;
            self.init_esc = PPMD7_K_EXP_ESCAPE[(prob >> 10) as usize] as u32;
            char_mask[self.st_symbol(self.ctx_one_state(self.min_context)) as usize] = 0;
            self.prev_success = 0;
        }

        loop {
            let mut ps = [0u32; 256];
            let num_masked = self.ctx_numstats(self.min_context) as u32;
            loop {
                self.order_fall += 1;
                if self.ctx_suffix(self.min_context) == 0 {
                    return -1;
                }
                self.min_context = self.ctx_suffix(self.min_context);
                if self.ctx_numstats(self.min_context) != num_masked as u16 {
                    break;
                }
            }
            let mut hi_cnt = 0u32;
            let mut s = self.ctx_stats(self.min_context);
            let mut i = 0usize;
            let num = self.ctx_numstats(self.min_context) as usize - num_masked as usize;
            loop {
                let k = char_mask[self.st_symbol(s) as usize] as i32;
                hi_cnt = (hi_cnt as i32).wrapping_add(self.st_freq(s) as i32 & k) as u32;
                ps[i] = s;
                s += 6;
                i = (i as i32 - k) as usize;
                if i == num {
                    break;
                }
            }

            let (see_idx, mut freq_sum) = self.make_esc_freq(num_masked);
            freq_sum += hi_cnt;
            let count = rc.get_threshold(freq_sum);

            if count < hi_cnt {
                let mut pps = 0usize;
                let mut h = 0u32;
                loop {
                    h += self.st_freq(ps[pps]) as u32;
                    if h > count {
                        break;
                    }
                    pps += 1;
                }
                let s = ps[pps];
                rc.decode(h - self.st_freq(s) as u32, self.st_freq(s) as u32, read);
                if see_idx != u32::MAX {
                    let (r, c) = (see_idx as usize / 16, see_idx as usize % 16);
                    let se = &mut self.see[r][c];
                    see_update(&mut se.shift, &mut se.summ, &mut se.count);
                }
                self.found_state = s;
                let symbol = self.st_symbol(s);
                self.update2();
                return symbol as i32;
            }
            if count >= freq_sum {
                return -2;
            }
            rc.decode(hi_cnt, freq_sum - hi_cnt, read);
            if see_idx != u32::MAX {
                let (r, c) = (see_idx as usize / 16, see_idx as usize % 16);
                self.see[r][c].summ = self.see[r][c].summ.wrapping_add(freq_sum as u16);
            }
            loop {
                i -= 1;
                char_mask[self.st_symbol(ps[i]) as usize] = 0;
                if i == 0 {
                    break;
                }
            }
        }
    }
}
