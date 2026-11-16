// Keccak-f[1600] worker for a Baochip-1x BIO core (PicoRV32, RV32E + Zmmul, 4 KiB RAM).
//
// Loop: pop the 200-byte state as 50 x u32 from FIFO0 (lane i = words 2i (low) and 2i+1
// (high), little-endian, as in FIPS 202), run the 24 rounds, push the 50 words to FIFO1.
//
// Constraints of the BIO: no divider (so no `%`), 15 general registers, the state lives in
// RAM. Tables replace the modular index arithmetic. Erratum patches for the generated
// assembly are applied by clang2rustasm.py, whose translator only handles word-sized
// tables and plain symbol addresses: tables are u32, and the state is reached through a
// pointer the compiler can't fold into `symbol+offset` addressing.

#include "bio.h"

static const uint32_t RC_LO[24] = {
    0x00000001, 0x00008082, 0x0000808A, 0x80008000, 0x0000808B, 0x80000001,
    0x80008081, 0x00008009, 0x0000008A, 0x00000088, 0x80008009, 0x8000000A,
    0x8000808B, 0x0000008B, 0x00008089, 0x00008003, 0x00008002, 0x00000080,
    0x0000800A, 0x8000000A, 0x80008081, 0x00008080, 0x80000001, 0x80008008,
};
static const uint32_t RC_HI[24] = {
    0x00000000, 0x00000000, 0x80000000, 0x80000000, 0x00000000, 0x00000000,
    0x80000000, 0x80000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000,
    0x00000000, 0x80000000, 0x80000000, 0x80000000, 0x80000000, 0x80000000,
    0x00000000, 0x80000000, 0x80000000, 0x80000000, 0x00000000, 0x80000000,
};
// rho offsets and pi lane order, in the order the rho-pi walk visits lanes
static const uint32_t ROTC[24] = {1, 3, 6, 10, 15, 21, 28, 36, 45, 55, 2, 14,
                                 27, 41, 56, 8, 25, 43, 62, 18, 39, 61, 20, 44};
static const uint32_t PILN[24] = {10, 7, 11, 17, 18, 3, 5, 16, 8, 21, 24, 4,
                                 15, 23, 19, 13, 12, 2, 20, 14, 22, 9, 6, 1};
// (i + 1) mod 5, (i + 2) mod 5, (i + 4) mod 5
static const uint32_t P1[5] = {1, 2, 3, 4, 0};
static const uint32_t P2[5] = {2, 3, 4, 0, 1};
static const uint32_t P4[5] = {4, 0, 1, 2, 3};

static uint64_t state[25];

// hide the address from constant folding (see above)
static inline uint64_t *opaque(uint64_t *p) {
    __asm__ volatile("" : "+r"(p));
    return p;
}

static inline uint64_t rol(uint64_t x, uint32_t n) {
    return (x << n) | (x >> (64 - n)); // n is 1..63 for every call site
}

static void keccakf(uint64_t *st) {
    uint64_t bc[5];
    uint64_t t;
    for (uint32_t r = 0; r < 24; r++) {
        // theta
        for (uint32_t i = 0; i < 5; i++) {
            bc[i] = st[i] ^ st[i + 5] ^ st[i + 10] ^ st[i + 15] ^ st[i + 20];
        }
        for (uint32_t i = 0; i < 5; i++) {
            t = bc[P4[i]] ^ rol(bc[P1[i]], 1);
            st[i] ^= t;
            st[i + 5] ^= t;
            st[i + 10] ^= t;
            st[i + 15] ^= t;
            st[i + 20] ^= t;
        }
        // rho + pi
        t = st[1];
        for (uint32_t i = 0; i < 24; i++) {
            uint32_t j = PILN[i];
            uint64_t tmp = st[j];
            st[j] = rol(t, ROTC[i]);
            t = tmp;
        }
        // chi
        for (uint32_t j = 0; j < 25; j += 5) {
            for (uint32_t i = 0; i < 5; i++) {
                bc[i] = st[j + i];
            }
            for (uint32_t i = 0; i < 5; i++) {
                st[j + i] ^= (~bc[P1[i]]) & bc[P2[i]];
            }
        }
        // iota
        st[0] ^= ((uint64_t)RC_HI[r] << 32) | RC_LO[r];
    }
}

void main(void) {
    uint64_t *st = opaque(state);
    uint32_t *w = (uint32_t *)st;
    while (1) {
        for (uint32_t i = 0; i < 50; i++) {
            w[i] = pop_fifo0();
        }
        keccakf(st);
        for (uint32_t i = 0; i < 50; i++) {
            push_fifo1(w[i]);
        }
    }
}
