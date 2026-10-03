; Sprite lists in any bank: Lunar Magic's sprite data banks.
;
; Kobo's own code, written from the vanilla code it hooks and the format the
; community documents (docs/lunar-magic.md), never from Lunar Magic's code.
;
; The game keeps a level's sprite list in bank $07, behind a 2-byte pointer
; at $05EC00. Lunar Magic's layout adds a bank byte per level at $0EF100,
; so a list can be anywhere, and a JSL over the game's LDA #$07 : STA $D0
; in the primary header load, where the pointer's bank is set. Lunar Magic
; counts this piece as installed when $05D8F5 is a JSL, whatever it calls,
; and a save then keeps the table (docs/lunar-magic-install.md).
;
; $0EF30C-$0EF30F are PIXI's (the sprite size table's pointer and its
; marker), so nothing here goes past $0EF30B.

incsrc "memory.asm"

; A 8-bit, X and Y 16-bit, $0E the level (16-bit).
org $05D8F5
    JSL sprite_bank

; Every list in bank $07, as the game has them, until a build moves one.
org $0EF100
    fillbyte $07 : fill $200

org $0EF300
sprite_bank:
    PHX
    LDX $0E
    LDA.l $0EF100,x
    STA $D0
    PLX
    RTL
assert pc() <= $0EF30C
