// Test core: a logic analyser for the display engine (board self-test only).
//
// Records the SPI bit stream on SCK (BIO 16) rising edges: MOSI (BIO 17) into 32-bit words
// (one store per 32 edges, so the sampling loop stays short), DC (BIO 18) once per byte. It
// starts at the first edge and stops when the bus has been idle for ~50 ms (or the buffer
// is full), then pushes to FIFO3: the bit count, the MOSI words, the DC words. The host
// compares the bit stream with the expected bytes, so a missing, extra or wrong bit shows up
// exactly.

#include "bio.h"

#define SCK  (1u << 16)
#define MOSI_SHIFT 17
#define DC_SHIFT 18
#define MAXBITS 16384u            // 2 KiB of MOSI bits
#define IDLE 35000000u            // ~50 ms at the 700 MHz BIO clock

static uint32_t bw[MAXBITS / 32];
static uint32_t dw[MAXBITS / 256];

// hide the addresses from constant folding (the translator only handles plain symbols)
static inline uint32_t *opaque(uint32_t *p) {
    __asm__ volatile("" : "+r"(p));
    return p;
}

int main(void) {
    uint32_t *b = opaque(bw);
    uint32_t *d = opaque(dw);
    set_gpio_mask(0);
    while (1) {
        uint32_t n = 0, acc = 0, dacc = 0;
        uint32_t prev = read_gpio_pins() & SCK;
        uint32_t last = aclk_counter();
        while (n < MAXBITS) {
            uint32_t p = read_gpio_pins();
            uint32_t s = p & SCK;
            if (s && !prev) {
                acc = (acc << 1) | ((p >> MOSI_SHIFT) & 1);
                n++;
                if ((n & 31) == 0) {
                    b[(n >> 5) - 1] = acc;
                }
                if ((n & 7) == 0) {
                    dacc = (dacc << 1) | ((p >> DC_SHIFT) & 1);
                    if ((n & 255) == 0) {
                        d[(n >> 8) - 1] = dacc;
                    }
                }
                last = aclk_counter();
            } else if (n != 0 && ((aclk_counter() - last) & 0x3FFFFFFFu) > IDLE) {
                break;
            }
            prev = s;
        }
        // flush the partial words, left-aligned
        if (n & 31) {
            b[n >> 5] = acc << (32 - (n & 31));
        }
        if ((n >> 3) & 31) {
            d[n >> 8] = dacc << (32 - ((n >> 3) & 31));
        }
        push_fifo3(n);
        for (uint32_t i = 0; i < (n + 31) >> 5; i++) {
            push_fifo3(b[i]);
        }
        for (uint32_t i = 0; i < (n + 255) >> 8; i++) {
            push_fifo3(d[i]);
        }
    }
    return 0;
}
