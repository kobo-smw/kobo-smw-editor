; The level number, where code that runs later finds it.
;
; Kobo's own code, written from the vanilla code it hooks and the memory a
; Lunar Magic-saved ROM's level load leaves (docs/lunar-magic-install.md),
; never from Lunar Magic's code. PIXI's own hook stores the same $010B.
;
; The hook replaces LDA $0E : ASL : TAY before the sprite pointer read in
; the primary header load, with A, X, and Y 16-bit and $0E the level.
; Lunar Magic keeps this piece when its area here is not empty, so the
; code sits at the address its layout has for it.

incsrc "memory.asm"

org $05D8E2
    JSL level_number

org $0EF550
level_number:
    LDA $0E
    STA $010B|!addr         ; the level
    INC A
    STA $00FE|!dp           ; the level plus one
    DEC A
    ASL A
    TAY                     ; as the instructions it replaces leave it
    RTL

; Not $FF at $0FF0A0, where Lunar Magic's save writes its version string:
; the retry system and others take a byte there to mean Lunar Magic's
; install is in, and refuse to assemble otherwise. One byte, not the
; string, so nothing reads a Kobo build as a version of Lunar Magic
; (docs/lunar-magic-install.md, "Bytes Kobo writes because Lunar Magic or
; other tools check them"); a save writes its string over it.
org $0FF0A0
    db $00
