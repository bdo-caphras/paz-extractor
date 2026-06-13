//! ICE block cipher (Information Concealment Engine), by Matthew Kwan.
//!
//! The original C reference by Matthew Kwan is in the public domain
//! (<https://www.darkside.com.au/ice/>). This is an independent, clean-room
//! Rust port written from the published algorithm description; it is not a
//! copy of any GPL/third-party BDO unpacker.
//!
//! Black Desert encrypts the per-archive filename table (and `.dbss` payloads
//! are stored *unencrypted*) with ICE in "level 1" mode (8 rounds, 8-byte key),
//! operating as an 8-byte ECB block cipher.
//!
//! The code below is a deliberately faithful port of the reference algorithm,
//! so a few clippy idioms (index-based loops, explicit shifts) are kept on
//! purpose for verifiability.
#![allow(
    clippy::needless_range_loop,
    clippy::manual_rotate,
    clippy::manual_is_multiple_of,
    clippy::unnecessary_cast
)]

/// Galois-field multiply used to build the S-boxes.
fn gf_mult(mut a: u32, mut b: u32, m: u32) -> u32 {
    let mut res = 0u32;
    while b != 0 {
        if b & 1 != 0 {
            res ^= a;
        }
        a <<= 1;
        b >>= 1;
        if a >= 256 {
            a ^= m;
        }
    }
    res
}

/// Galois-field exponentiation to the 7th power.
fn gf_exp7(b: u32, m: u32) -> u32 {
    if b == 0 {
        return 0;
    }
    let mut x = gf_mult(b, b, m);
    x = gf_mult(b, x, m);
    x = gf_mult(x, x, m);
    gf_mult(b, x, m)
}

/// The 32-bit permutation applied while initialising the S-boxes.
fn ice_perm32(mut x: u32) -> u32 {
    const PBOX: [u32; 32] = [
        0x0000_0001,
        0x0000_0080,
        0x0000_0400,
        0x0000_2000,
        0x0008_0000,
        0x0020_0000,
        0x0100_0000,
        0x4000_0000,
        0x0000_0008,
        0x0000_0020,
        0x0000_0100,
        0x0000_4000,
        0x0001_0000,
        0x0080_0000,
        0x0400_0000,
        0x2000_0000,
        0x0000_0004,
        0x0000_0010,
        0x0000_0200,
        0x0000_8000,
        0x0002_0000,
        0x0040_0000,
        0x0800_0000,
        0x1000_0000,
        0x0000_0002,
        0x0000_0040,
        0x0000_0800,
        0x0000_1000,
        0x0004_0000,
        0x0010_0000,
        0x0200_0000,
        0x8000_0000,
    ];
    let mut res = 0u32;
    let mut i = 0usize;
    while x != 0 {
        if x & 1 != 0 {
            res |= PBOX[i];
        }
        i += 1;
        x >>= 1;
    }
    res
}

const SMOD: [[u32; 4]; 4] = [
    [333, 313, 505, 369],
    [379, 375, 319, 391],
    [361, 445, 451, 397],
    [397, 425, 395, 505],
];
const SXOR: [[u32; 4]; 4] = [
    [0x83, 0x85, 0x9b, 0xcd],
    [0xcc, 0xa7, 0xad, 0x41],
    [0x4b, 0x2e, 0xd4, 0x33],
    [0xea, 0xcb, 0x2e, 0x04],
];
const KEYROT: [u32; 16] = [0, 1, 2, 3, 2, 1, 3, 0, 1, 3, 2, 0, 3, 1, 0, 2];

/// A single ICE key, holding the expanded S-boxes and round subkeys.
pub struct IceKey {
    rounds: usize,
    sbox: Box<[[u32; 1024]; 4]>,
    keysched: Vec<[u32; 3]>,
}

impl IceKey {
    /// Build a key. `key_len_bytes` must be 8 (level 1) or a multiple of 16.
    pub fn new(key: &[u8]) -> Self {
        let key_len = key.len();
        let (key_size, rounds) = if key_len == 8 {
            (1usize, 8usize)
        } else {
            assert!(
                key_len % 16 == 0,
                "ICE key length must be 8 or a multiple of 16"
            );
            let n = key_len / 16;
            (n, n * 16)
        };

        let mut me = IceKey {
            rounds,
            sbox: Box::new([[0u32; 1024]; 4]),
            keysched: vec![[0u32; 3]; rounds],
        };
        me.sbox_init();
        me.key_set(key, key_size);
        me
    }

    fn sbox_init(&mut self) {
        for i in 0..1024usize {
            let col = ((i >> 1) & 0xff) as u32;
            let row = ((i & 0x1) | ((i & 0x200) >> 8)) as usize;

            let x = gf_exp7(col ^ SXOR[0][row], SMOD[0][row]) << 24;
            self.sbox[0][i] = ice_perm32(x);
            let x = gf_exp7(col ^ SXOR[1][row], SMOD[1][row]) << 16;
            self.sbox[1][i] = ice_perm32(x);
            let x = gf_exp7(col ^ SXOR[2][row], SMOD[2][row]) << 8;
            self.sbox[2][i] = ice_perm32(x);
            let x = gf_exp7(col ^ SXOR[3][row], SMOD[3][row]);
            self.sbox[3][i] = ice_perm32(x);
        }
    }

    fn key_sched_build(&mut self, kb: &mut [u16; 4], n: usize, keyrot: &[u32]) {
        for i in 0..8usize {
            let kr = keyrot[i] as usize;
            let isk = n + i;
            self.keysched[isk] = [0; 3];

            for j in 0..15usize {
                for k in 0..4usize {
                    let curr_kb = &mut kb[(kr + k) & 3];
                    let bit = (*curr_kb & 1) as u32;
                    self.keysched[isk][j % 3] = (self.keysched[isk][j % 3] << 1) | bit;
                    *curr_kb = (*curr_kb >> 1) | (((bit ^ 1) as u16) << 15);
                }
            }
        }
    }

    fn key_set(&mut self, key: &[u8], key_size: usize) {
        let mut kb = [0u16; 4];
        if self.rounds == 8 {
            for i in 0..4 {
                kb[3 - i] = ((key[i * 2] as u16) << 8) | key[i * 2 + 1] as u16;
            }
            self.key_sched_build(&mut kb, 0, &KEYROT);
        } else {
            for i in 0..key_size {
                for j in 0..4 {
                    kb[3 - j] = ((key[i * 8 + j * 2] as u16) << 8) | key[i * 8 + j * 2 + 1] as u16;
                }
                self.key_sched_build(&mut kb, i * 8, &KEYROT);
                let rounds = self.rounds;
                self.key_sched_build(&mut kb, rounds - 8 - i * 8, &KEYROT[8..]);
            }
        }
    }

    #[inline]
    fn round_f(&self, p: u32, sk: &[u32; 3]) -> u32 {
        let tl = ((p >> 16) & 0x3ff) | (((p >> 14) | (p << 18)) & 0xf_fc00);
        let tr = (p & 0x3ff) | ((p << 2) & 0xf_fc00);

        let mut al = sk[2] & (tl ^ tr);
        let ar = al ^ tr;
        al ^= tl;

        al ^= sk[0];
        let ar = ar ^ sk[1];

        self.sbox[0][(al >> 10) as usize]
            | self.sbox[1][(al & 0x3ff) as usize]
            | self.sbox[2][(ar >> 10) as usize]
            | self.sbox[3][(ar & 0x3ff) as usize]
    }

    /// Decrypt a single 8-byte block in place.
    fn decrypt_block(&self, block: &mut [u8; 8]) {
        let mut l = u32::from_be_bytes([block[0], block[1], block[2], block[3]]);
        let mut r = u32::from_be_bytes([block[4], block[5], block[6], block[7]]);

        let mut i = self.rounds as isize - 1;
        while i > 0 {
            l ^= self.round_f(r, &self.keysched[i as usize]);
            r ^= self.round_f(l, &self.keysched[(i - 1) as usize]);
            i -= 2;
        }

        block[0..4].copy_from_slice(&r.to_be_bytes());
        block[4..8].copy_from_slice(&l.to_be_bytes());
    }

    /// Decrypt `data` (length must be a multiple of 8) in place, ECB mode.
    pub fn decrypt(&self, data: &mut [u8]) {
        debug_assert!(
            data.len() % 8 == 0,
            "ICE input must be a multiple of 8 bytes"
        );
        let mut block = [0u8; 8];
        for chunk in data.chunks_exact_mut(8) {
            block.copy_from_slice(chunk);
            self.decrypt_block(&mut block);
            chunk.copy_from_slice(&block);
        }
    }
}
