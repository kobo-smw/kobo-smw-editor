; Lunar Magic's overworld palettes: a palette of 256 colours for each map,
; and another for each once Special World is passed, 14 in a block the
; build writes, in place of the game's colours for the overworld
; (docs/lunar-magic-install.md, "The overworld").
;
; Kobo's own code, written from the game's routine it takes the place of
; (CODE_00AD25, which loads the overworld's colours into MainPalette by the
; submap and whether Special World is passed) and what Lunar Magic-saved
; ROMs leave in RAM and CGRAM after the overworld's load; never from Lunar
; Magic's code. Applied after overworld.asm, for a project with palettes.
;
; The block's pointer is in the code below, its low word 18 bytes from the
; start and its bank 30, where Lunar Magic's layout keeps them; the build
; writes them.

incsrc "memory.asm"

!main_palette = $0703|!addr
!back_area = $0701|!addr
!passed_special = $1EEA|!addr   ; translevel $48's settings: bit 7, passed
!player_turn = $0DB3|!addr
!player_submaps = $1F11|!addr

; In CODE_00AD25, from its STY $00 on: the whole palette, and the game's
; routine returns.
org $00AD32
    JSL overworld_palette
    RTS

freecode
overworld_palette:
    BRA .run
    fillbyte $FF
    fill $12-2
.low:
    dw $FFFF
    fill $1E-$14
.bank:
    db $FF
.run:
    ; A, X, and Y 16-bit, as CODE_00AD25 has them here.
    PHB
    ; The palette: the player's submap's, 7 on once Special World is
    ; passed, 512 bytes each.
    LDA !player_turn
    AND #$00FF
    TAX
    LDA !player_submaps,x
    AND #$00FF
    BIT !passed_special-1
    BPL +
    CLC
    ADC #$0007
+   XBA
    ASL A
    CLC
    ADC.l .low
    TAY
    SEP #$20
    LDA.l .bank
    PHA
    PLB
    REP #$20
    ; From the block (Y, its bank the data bank) to MainPalette (X), which
    ; a long store reaches whatever the bank.
    LDX #$0000
-   LDA $0000,y
    STA.l !main_palette,x
    INY
    INY
    INX
    INX
    CPX #$0200
    BNE -
    PLB
    LDA !main_palette
    STA !back_area
    ; A, X, and Y 8-bit, as the game's routine leaves them.
    SEP #$30
    RTL

assert overworld_palette_low-overworld_palette == $12
assert overworld_palette_bank-overworld_palette == $1E
