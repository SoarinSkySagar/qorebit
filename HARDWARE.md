# Hardware: pin connections and board bring-up

Qorebit runs on the **Baochip-1x Dabao** board: open-source hardware (the board's KiCad files and the chip's RTL are CERN-OHL-W-2.0, `github.com/baochip/dabao` and `github.com/baochip/baochip-1x`) around a chip designed to be verified (README, "Open hardware and a verifiable chip"). To check your own board's boot code against Baochip's published hashes, put it in its bootloader and run `make audit` (read-only). There are two ways to use it, and only the second needs any wiring.

| Mode | Screen | Button | Wiring |
|---|---|---|---|
| **Virtual display** (`make flash-virtual`, `make board-virtual`) | a window on your laptop | the two buttons in the window | none: USB-C only |
| **Panel** (`make flash-panel`, `make board-panel`) | an ST7789 240x280 (1.69") SPI display on the board | two buttons on **PB1** (left) and **PB2** (right) | the table below |

## Pin connections for the panel build

All signals are 3.3 V logic (the chip's I/O level). Use a 3.3 V ST7789 module, and power it from the board's 3V3 pin.
Do not connect it to 5 V.

| Dabao pin | Board pin no. and side | Function | Goes to | Notes |
|---|---|---|---|---|
| **PC0** | 14, left | SPI2 SCK | display **SCL** (SCK) | clock |
| **PC1** | 12, left | SPI2 MOSI | display **SDA** (MOSI) | data; the display is write-only, no MISO |
| **PC2** | 11, left | GPIO output | display **DC** (A0 / RS) | low = command, high = data |
| **PC3** | 10, left | SPI2 CS0 | display **CS** | chip select |
| **PB4** | 34, right | GPIO output | display **RES** (RST) | reset, pulsed at start |
| **PB5** | 35, right | GPIO output | display **BLK** (BL / LED) | backlight, driven high |
| **PB1** | 29, right | GPIO input, internal pull-up | one side of the **button** | pressed = low |
| **GND** | 13 (left), 33 (right) | ground | display **GND**, the other side of the **button** | any GND pin works |
| **3V3** | 36, right | 3.3 V supply | display **VCC** | |

**Where the pins are.** The board pin numbers and the left / right grouping come from the Dabao repo's pinout
(`docs/pinout/pins.csv` and its README, <https://github.com/baochip/dabao/tree/main/docs/pinout>). Left side: pins 1 to 16,
numbered top to bottom, so PC3 is the 10th pin down, PC2 the 11th, PC1 the 12th, PC0 the 14th. Right side: pins 25 to 40,
numbered top to bottom with pin **40 at the top**, so 3V3 is the 5th pin down, PB5 the 6th, PB4 the 7th and PB1 the 12th.
Those docs do not say which physical edge of the board is "left", so identify it from the board's own silkscreen or
from `dabao-front.png` in that folder, and check one pin (for example GND or 3V3 with a meter) before wiring the rest.

The two buttons, each a normal momentary switch to GND, no external resistor (the chip's pull-ups are on):
**left = PB1 to GND = 0 = reject**, **right = PB2 to GND = 1 = approve**. One press is the answer.

```
 Dabao                         ST7789 (240x280)
 PC0  ───────────────────────  SCL
 PC1  ───────────────────────  SDA
 PC2  ───────────────────────  DC
 PC3  ───────────────────────  CS
 PB4  ───────────────────────  RES
 PB5  ───────────────────────  BLK
 3V3  ───────────────────────  VCC
 GND  ───────────────────────  GND

 PB1  ──┤ button ├── GND
```

Signal names differ between ST7789 breakouts (SCL/SCK, SDA/MOSI, DC/A0/RS, RES/RST, BLK/BL/LED): match them by function.
Check the pin labels on your own Dabao breakout against the chip pin names (PC0, PB1 and so on) before powering up.

Source of truth: `firmware/dabao-app-qorebit/src/panel.rs` (the constants at the top). The SPI clock is 4 MHz over the uDMA
path and 2 MHz over the BIO engine, which is slow on purpose for breadboard jumpers; keep wires short and raise the
clock in `panel.rs` once it works.

## Bring-up on the board, in order

Do these in order. Each step checks one thing, so a failure points at one cause.

1. **Board alone, virtual display** (no wiring, USB-C to the laptop):
   ```bash
   make flash-virtual    # when asked: unplug the Dabao, hold PROG, plug it back in
   make board-virtual    # the window + website
   ```
   Check: the website shows the device, the window shows the QOREBIT screen, and the two buttons in the window work
   (left rejects, right approves). `make info` shows the firmware and `make security` the chip state.
2. **Signing memory.** In the website, set the PIN, register the post-quantum key and send once. This is the first time
   ML-DSA-87 (about 350 KiB of stack, in a 416 KiB thread) runs on the real 2 MiB chip. If it fails or the board resets
   here, tell us. (It has worked on the author's board.)
3. **Display and button engines without the panel:**
   ```bash
   make bio-test
   ```
4. **Wire the panel** (table above), power the board, then:
   ```bash
   make flash-panel
   make board-panel
   ```
   Check: the QOREBIT screen appears on the display, PB1 (left) and PB2 (right) both work (`info` shows the pin level and the count of
   presses), then repeat step 2 using only the display and the button.

Blank display: check VCC/GND, then RES and BLK, then lower the clock. Wrong colours or a shifted image: check DC and
CS. Button does nothing: check PB1 to GND with a meter (it should read low when pressed).

The panel build has been seen running on a board (home screen with the address). Signing on the chip has been run (testnet transfers, Cross-VM,
smart accounts); the panel's own display and the PB1 / PB2 buttons are the least tested part, so report what you see.
