// Test core: presses the buttons electrically (board self-test only).
//
// Pops commands from FIFO2: bit 17 = hold the right button (BIO 2, PB2) low,
// bit 16 = hold the left button (BIO 1, PB1) low, bits 15:0 = for how many quanta
// (1 ms each). A released button goes back to input, where the pull-up makes it high, as
// a real button does. The button engine owns FIFO3, so the end of each command is signalled
// through event bit 0 (the host waits on it).

#include "bio.h"

#define LEFT  (1u << 1)
#define RIGHT (1u << 2)

int main(void) {
    set_gpio_mask(LEFT | RIGHT);
    set_input_pins(LEFT | RIGHT);
    while (1) {
        uint32_t w = pop_fifo2();
        uint32_t low = ((w & (1u << 17)) ? RIGHT : 0) | ((w & (1u << 16)) ? LEFT : 0);
        // pressed: drive low; released: input (pulled up)
        clear_gpio_pins_n(~low);
        set_output_pins(low);
        set_input_pins((LEFT | RIGHT) & ~low);
        for (uint32_t t = w & 0xffff; t != 0; t--) {
            wait_quantum();
        }
        set_event_bits(1);
    }
    return 0;
}
