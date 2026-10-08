; Lunar Magic's overworld layout: the overworld's tables where its layout
; keeps them (docs/lunar-magic-install.md, "The overworld"), which
; kobo_core::overworld writes.
;
; Kobo's own code, written from the vanilla code it replaces (SMWDisX) and
; what a Lunar Magic-saved ROM's overworld leaves in RAM when it loads,
; never from Lunar Magic's code.
;
; This piece is the load's: the game numbered translevels by scanning
; layer 1 for level tiles (CODE_04D7F2); in this layout they are stored,
; with each tile's direction byte, and layer 1 has a second page. So the
; scan is replaced: both tables are unpacked where the game's scan would
; have left them, and the game's own loop through the events at the load
; (from $04D84F) goes on as before. The tables the game's own code reads
; elsewhere the build moves by its instructions' operands.
;
; The pointers this code reads, at the places Lunar Magic's layout keeps
; them, which the build writes: the translevels and directions, LC_LZ2,
; $1000 bytes, low word at $04D803 and bank at $04D808; layer 1's page
; bytes, LC_LZ2, $800 bytes, low word at $04D822 and bank at $04D827.

incsrc "memory.asm"

; The game's LC_LZ2 decompressor, CODE_00B8DE: $8A-$8C the source, $00-$02
; the destination; ends with RTS, so it is entered through an RTL of the
; game's in bank $00 (at $0084CF) as its return.
!decompress = $00B8DE
!rtl_00 = $0084CF

org $04D7F9
    JML load_tables

; The pointers, among bytes nothing runs: the JML above leaves the rest of
; the scan's place unused.
org $04D803 : dw $FFFF          ; the translevels' low word
org $04D808 : db $FF            ; their bank
org $04D822 : dw $FFFF          ; layer 1's pages' low word
org $04D827 : db $FF            ; their bank

freecode
; Entered as CODE_04D7F2 left the scan: A 8-bit, X and Y 16-bit. Leaves
; the same for the game's loop through the events at $04D84F.
load_tables:
    PHB
    PHK
    PLB
    REP #$20
    LDA.l $04D803
    STA $8A
    LDA.w #!map16_low+$800
    STA $00
    SEP #$20
    LDA.l $04D808
    STA $8C
    LDA.b #(!map16_low+$800)>>16
    STA $02
    JSL unpack
    REP #$20
    LDA.l $04D822
    STA $8A
    LDA.w #!map16_high
    STA $00
    SEP #$20
    LDA.l $04D827
    STA $8C
    LDA.b #!map16_high>>16
    STA $02
    JSL unpack
    PLB
    SEP #$20
    REP #$10
    JML $04D84F

; The game's decompressor, from code outside bank $00: $8A-$8C the source,
; $00-$02 the destination. Its RTS returns to !rtl_00, whose RTL returns
; here.
unpack:
    PHB
    PHP
    SEP #$20
    LDA #$00
    PHA
    PLB
    PHK
    PER .back-1
    PEA !rtl_00-1
    JML !decompress
.back:
    PLP
    PLB
    RTL
