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
