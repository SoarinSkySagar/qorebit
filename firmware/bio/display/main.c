// ST7789 display engine for a Baochip-1x BIO core.
//
// Takes compact drawing commands from the host on FIFO2 and clocks them out as SPI mode 0
// (MSB first) on the BIO pins, so the main CPU never touches a pixel bit:
//
//   SCK = BIO 16 (PC0), MOSI = BIO 17 (PC1), DC = BIO 18 (PC2), CS = BIO 19 (PC3)
//
// Command words (bits 31:28 = opcode):
//   0x1 cmd   : bits 7:0 sent with DC low (a panel command)
//   0x2 data  : bits 7:0 sent with DC high (a parameter)
//   0x3 run   : bits 19:16 palette index, bits 15:0 count: count pixels of that colour,
//               each the palette's RGB565 value, high byte first (DC high)
//   0x4 pal   : bits 19:16 palette index, bits 15:0 RGB565 value
//   0x6 sync  : echo the word on FIFO3 once everything before it is on the wire
//
// The palette expansion is the point: the host sends 4-bit colour indices in runs (the
// firmware's frames are run-length encoded 16-colour images), and only this core turns
// them into the 16 bits per pixel the panel wants. Each half SCK period waits for the
// core's quantum, so the host sets the SPI clock with the core's clock divider
// (SCK = quantum / 2).

#include "bio.h"

#define SCK  (1u << 16)
#define MOSI (1u << 17)
#define DC   (1u << 18)
#define CS   (1u << 19)
#define PINS (SCK | MOSI | DC | CS)

static void send_byte(uint32_t b) {
    for (uint32_t m = 0x80; m != 0; m >>= 1) {
        if (b & m) {
            set_gpio_pins(MOSI);
        } else {
            clear_gpio_pins_n(~MOSI);
        }
        wait_quantum();
        set_gpio_pins(SCK);
        wait_quantum();
        clear_gpio_pins_n(~SCK);
    }
}

int main(void) {
    uint32_t pal[16];
    for (uint32_t i = 0; i < 16; i++) {
        pal[i] = 0;
    }
    set_gpio_mask(PINS);
    // idle levels first (SCK low, MOSI low, the panel selected, data mode), then drive the
    // pins: enabling the outputs first could put a stray clock pulse on SCK, which the panel
    // would read as a bit (the board self-test caught exactly that)
    clear_gpio_pins_n(~(SCK | MOSI | CS));
    set_gpio_pins(DC);
    set_output_pins(PINS);
    while (1) {
        uint32_t w = pop_fifo2();
        uint32_t op = w >> 28;
        if (op == 1) {
            clear_gpio_pins_n(~DC);
            send_byte(w & 0xff);
            set_gpio_pins(DC);
        } else if (op == 2) {
            send_byte(w & 0xff);
        } else if (op == 3) {
            uint32_t c = pal[(w >> 16) & 15];
            uint32_t hi = c >> 8;
            uint32_t lo = c & 0xff;
            for (uint32_t n = w & 0xffff; n != 0; n--) {
                send_byte(hi);
                send_byte(lo);
            }
        } else if (op == 4) {
            pal[(w >> 16) & 15] = w & 0xffff;
        } else if (op == 6) {
            push_fifo3(w);
        }
    }
    return 0;
}
