; Backgrounds in Lunar Magic's formats, and BG Map16 past the game's own.
;
; Kobo's own code, written from the vanilla code it hooks (SMWDisX), the
; formats the community documents (docs/lunar-magic.md), and what a Lunar
; Magic-saved ROM's level load leaves in memory (docs/lunar-magic-install.md,
; "Backgrounds and the level load's uploads"), never from Lunar Magic's code.
;
; A level's flags at $0EF310 are bbBBVFCT: C a background in Lunar Magic's
; own format behind the level's full layer 2 pointer, F with its high bytes
; (32 rows to a half, 2048 bytes: the low bytes of both halves, then the
; high bytes), V the game's format (27 rows, 864 low bytes) behind a full
; pointer. bbBB is the BG Map16 table of Lunar Magic's own format, one of
; the 16 pointers at $0EFD50; without F (C alone, from older versions, 864
; low bytes) and with V it is every tile's high byte instead, the table
; being the first. A pointer with bank $FF is the game's format in bank $0C
; whatever the flags say.
;
; Lunar Magic counts the background piece and the flags as installed when
; $0EF519 holds a JML ($5C), and the BG Map16 piece and its pointers when
; $058DA4 is a JSL (docs/lunar-magic-install.md, "How a save decides what
; to install"). A save rewrites $0EF510-$0EF54F with its own code and frees
; the block the JML at $0EF519 leads to, so that block holds only the
; background code; the BG Map16 routine has a block of its own.

incsrc "memory.asm"

!flags = $7FC00B                ; the level's flags, for code that runs later
!custom = $02                   ; C
!high_bytes = $04               ; F
!vanilla = $08                  ; V

; ------------------------------------------------------------------------
; Tables, as a fresh install has them until a build writes its own.

; The flags: none for any level, so every level loads as the game has it.
org $0EF310
    fillbyte $00 : fill $200

; The BG Map16 tables: the game's first, the rest none.
org $0EFD50
    dl $0D9100                  ; Map16BGTiles
    fillbyte $00 : fill 15*3

; ------------------------------------------------------------------------
; The background load, in CODE_05801E: CMP #$FF : BNE CODE_058074 on the
; layer 2 pointer's bank, with A 8-bit holding it, X and Y 16-bit, and the
; level number in $010B (level.asm's hook, which runs before).
org $05803B
    JML background_entry

; The entry, where Lunar Magic's layout has it, laid out so that its JML is
; at $0EF519, the byte Lunar Magic checks.
org $0EF510
background_entry:
    XBA                         ; the bank to B
    LDX $010B|!addr             ; the level
    LDA.l $0EF310,x             ; its flags
    TAX                         ; X: the bank, then the flags
assert pc() == $0EF519
    autoclean JML background
assert pc() <= $0EF550          ; level.asm's code follows

; ------------------------------------------------------------------------
; The background column upload, CODE_058D7A, with A 16-bit and X and Y
; 8-bit: the BG Map16 table, and the bytes a background screen takes, are
; the level's, from the routine at $0EFD00, which also leaves A as the
; LDA.w $1928 after the STA $0A it replaces would.
org $058DA4
    JSL bg_map16_entry          ; STA $0A : LDA.w $1928
    NOP

; The right half starts a screen on: ADC #$01B0, twice.
org $058DB1
    ADC $05 : NOP
org $058DB9
    ADC $05 : NOP

; LDY #$0D : STY $0C: the bank stays the one the routine left.
org $058DCA
    NOP #2

; The column ends with the screen: CMP #$01B0.
org $058E12
    CMP $05 : NOP

; The routine's fixed entry, which Lunar Magic's layout and Kobo's VRAM
; patch (vram.asm) call.
org $0EFD00
bg_map16_entry:
    JML bg_map16
assert pc() <= $0EFD40

; ------------------------------------------------------------------------
freecode

; A 8-bit: the level's flags; X: the layer 2 pointer's bank in the high
; byte, the flags in the low; Y 16-bit. Every way out is to the game's
; code, as the instructions the hook replaces would have gone on.
background:
    STA.l !flags
    CPX #$FF00
    BCS .game                   ; bank $FF: the game's format
    BIT #!custom
    BEQ .not_custom
    BIT #!high_bytes
    BEQ .one_high_byte
    ; Lunar Magic's own with high bytes: the stream fills both buffers,
    ; $7EB900 and $7EBD00 after it.
    JML $058064

.not_custom:
    BIT #!vanilla
    BEQ .objects
    ; Low bytes only, the game's format behind a full pointer or Lunar
    ; Magic's own without high bytes: every tile's high byte is the flags'
    ; high nibble, in both halves' high bytes.
.one_high_byte:
    LSR #4
    LDX #$03FF
-   STA.l $7EBD00,x
    DEX
    BPL -
    ; On to the game's decode of the level's stream into $7EB900 (the
    ; object tileset cleared, as for the game's own backgrounds).
    JML $058064

.objects:
    JML $058074                 ; layer 2 objects, loaded with layer 1

.game:
    JML $05803F                 ; the game's background in bank $0C

; ------------------------------------------------------------------------
freecode

; The level's BG Map16 table in $0A-$0C, and in $05-$06 the bytes a
; background screen takes in the buffers. Lunar Magic's own format with
; high bytes (C with F, 32 rows): the pointer the flags' high nibble picks,
; and $0200. Any other background (the game's format, V, or Lunar Magic's
; older C alone, whose tiles carry the nibble as their high byte): the first
; pointer, and $01B0 (27 rows). Then A is $1928, as the instructions the
; hook replaces leave it, in the caller's accumulator size. Keeps the
; caller's register sizes, X, and Y.
bg_map16:
    PHP
    REP #$30
    PHX
    LDA.w #$01B0
    STA $05
    LDX.w #$0000                ; the first table
    LDA.l !flags
    AND.w #!custom|!high_bytes
    CMP.w #!custom|!high_bytes
    BNE +
    LDA.w #$0200
    STA $05
    LDA.l !flags
    AND.w #$00F0
    LSR #4
    STA $0A
    ASL A
    ADC $0A                     ; the table times 3 (no carry from the ASL)
    TAX
+   LDA.l $0EFD50,x
    STA $0A
    LDA.l $0EFD51,x
    STA $0B                     ; and its bank in $0C
    PLX
    PLP
    LDA.l $001928|!addr
    RTL
