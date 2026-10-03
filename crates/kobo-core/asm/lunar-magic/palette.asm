; Lunar Magic's custom level palettes.
;
; Kobo's own code, written from the format the community documents
; (docs/lunar-magic.md) and the vanilla code it hooks, and checked against
; the colours a Lunar Magic-saved ROM's levels load, never from Lunar
; Magic's code.
;
; A level's 3-byte pointer at $0EF600 leads to $202 bytes: the back area
; colour, then all 256 colours; $000000 or $FFFFFF for none. The hook
; replaces the JSL after the game assembles the level's palette in RAM
; (LoadPalette), in the level's setup (game mode $12), and copies the
; custom palette over it, before the game uploads it. It clears $00FE,
; which the level number hook sets to the level plus one, as a Lunar
; Magic-saved ROM's load leaves it wherever the setup runs this hook.

incsrc "memory.asm"

org $00A5BF
    autoclean JSL custom_palette    ; JSL CODE_05BE8A

; The palette's fade at a level's end (CODE_00AF35), as it comes out in a
; Lunar Magic-saved ROM (call_in_level, every step): colours 1 to 15 of rows 0 to 7 fade, but for layer 3's in rows 0
; and 1 (colours 9 to 11 of row 0, 9 to 11 and 13 to 15 of row 1), which
; stay as the level has them; the back area colour fades. The game's fades
; colours 1 to 7 of those rows and 13 to 15 of row 0
; (docs/lunar-magic-install.md, "Custom palettes"). The game's loop, from
; row 7's colour 15 down, 15 colours a row, stepping over each colour 0;
; then layer 3's colours are put back, where the game's loop for row 0's
; 13 to 15 was. With "LM" at $05DD7C (entrance.asm) Lunar Magic's save
; keeps these.
org $00AF4B
    LDX.w #$00FE                    ; #$00EE
org $00AF4E
    LDA.w #$000F                    ; #$0007
org $00AF6B
    SBC.w #$0002                    ; #$0012
org $00AF88
fade_background:                    ; LDA BackgroundColor
org $00AF71
    JSL fade_layer3                 ; LDX.W #$0004 : LDA.W CopyPalette+$1A,X
    BRA fade_background

freecode

; Layer 3's colours in rows 0 and 1 as the level has them (MainPalette)
; in the fade's copy (CopyPalette). A and X 16-bit, the data bank the
; game's fade has.
fade_layer3:
    LDX.w #$0004
-   LDA.w !addr|$0715,x             ; MainPalette+$12: row 0's 9 to 11
    STA.w !addr|$0917,x             ; CopyPalette+$12
    LDA.w !addr|$0735,x             ; row 1's 9 to 11
    STA.w !addr|$0937,x
    LDA.w !addr|$073D,x             ; row 1's 13 to 15
    STA.w !addr|$093F,x
    DEX
    DEX
    BPL -
    RTL

custom_palette:
    PHP
    REP #$30
    LDA $010B|!addr                 ; the level (level.asm)
    ASL A
    CLC
    ADC $010B|!addr
    TAX
    LDA.l $0EF600,x
    STA $00
    LDA.l $0EF601,x
    STA $01
    CMP #$FFFF
    BNE +
    LDA $00
    CMP #$FFFF
    BEQ .done                       ; $FFFFFF: none
    LDA $01
+:
    ORA $00
    BEQ .done                       ; $000000: none
    LDX #$0000
-   TXY
    LDA [$00],y
    STA.l $000701|!addr,x           ; BackgroundColor, then MainPalette
    INX
    INX
    CPX #$0202
    BNE -
.done:
    STZ $00FE|!dp                   ; as a Lunar Magic-saved ROM's load leaves it
    PLP
    JML $05BE8A                     ; what the hook replaces
