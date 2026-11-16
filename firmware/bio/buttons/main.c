// Button engine for a Baochip-1x BIO core: the wallet's two physical buttons.
//
//   left  (Reject / 0) = BIO 1 (PB1)
//   right (Approve / 1) = BIO 2 (PB2)
//
// BIO n is pad PBn for n < 16 and PC(n-16) above (baochip/dabao docs/pinout/pins.csv).
//
// Both are active low (to GND, with the chip's pull-ups). The core samples them once per
// quantum (the host sets 1 kHz: one tick per millisecond), so the timing is exact however
// busy the main CPU is (signing keeps it fully busy for seconds). A level only counts once
// it has been stable for DEBOUNCE ticks, so contact bounce and short glitches never make a
// press. On each debounced release it pushes one event to FIFO3:
//
//   bit 31 = 1, bit 24 = button (0 left, 1 right), bits 23:0 = how long it was held (ms)
//
// The host decides what a press means (a PIN bit, Approve, a hold-to-approve threshold).

#include "bio.h"

#define LEFT  (1u << 1)
#define RIGHT (1u << 2)
#define DEBOUNCE 10
#define MAX_HELD 0xffffffu

int main(void) {
    set_gpio_mask(LEFT | RIGHT);
    set_input_pins(LEFT | RIGHT);
    uint32_t stable[2] = {0, 0};   // debounced state: 1 = pressed
    uint32_t count[2] = {0, 0};    // ticks the raw level has differed from `stable`
    uint32_t held[2] = {0, 0};     // ticks since the debounced press
    while (1) {
        wait_quantum();
        uint32_t pins = read_gpio_pins();
        for (uint32_t b = 0; b < 2; b++) {
            uint32_t pressed = (pins & (b ? RIGHT : LEFT)) == 0;
            if (stable[b] && held[b] < MAX_HELD) {
                held[b]++;
            }
            if (pressed != stable[b]) {
                count[b]++;
                if (count[b] >= DEBOUNCE) {
                    stable[b] = pressed;
                    count[b] = 0;
                    if (pressed) {
                        // both edges are seen DEBOUNCE ticks late, so counting from here
                        // gives the true hold time
                        held[b] = 0;
                    } else {
                        push_fifo3(0x80000000u | (b << 24) | held[b]);
                    }
                }
            } else {
                count[b] = 0;
            }
        }
    }
    return 0;
}
