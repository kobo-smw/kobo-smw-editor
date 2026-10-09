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
;
; The events' further tiles (CODE_04E9F1) are the other piece: the game
; goes through a list of 44 entries for an event's; this layout keeps each
; event's entries together, in tables of any length, which Kobo's code
; below goes through, at the list's place and in the layer 2 load's loop
; through the events. Its pointers are at fixed offsets from the code's
; start, where the layout keeps them.

incsrc "memory.asm"

; The game's LC_LZ2 decompressor, CODE_00B8DE: $8A-$8C the source, $00-$02
; the destination; ends with RTS, so it is entered through an RTL of the
; game's in bank $00 (at $0084CF) as its return.
!decompress = $00B8DE
!rtl_00 = $0084CF
; CODE_04E4A9, which puts an event's layer 2 block (Y its data, $04 its
; place, X and Y 16-bit) and ends with RTS, entered through an RTL of the
; game's in bank $04 (DecompressOverworldL2's, at $04DAB2).
!make_block = $04E4A9
!rtl_04 = $04DAB2
; The events passed (a bit each, OWEventsActivated) and the overworld's
; event process's state (OverworldEventProcess).
!events_passed = $1F02|!addr
!event_process = $1B86|!addr

; Lunar Magic reads the translevels through the pointer below only when
; the scan's place starts with $A2 (LDX #), and layer 1's pages only when
; $04D818 holds $A2 as well (docs/lunar-magic-install.md, "The
; overworld"): the LDX here is Kobo's, giving load_tables the first
; table's destination.
org $04D7F9
    LDX.w #(!map16_low+$800)&$FFFF
    JML load_tables

; The pointers, among bytes nothing runs: the JML above leaves the rest of
; the scan's place unused.
org $04D803 : dw $FFFF          ; the translevels' low word
org $04D808 : db $FF            ; their bank
org $04D818 : db $A2            ; Lunar Magic's check, for layer 1's pages
org $04D822 : dw $FFFF          ; layer 1's pages' low word
org $04D827 : db $FF            ; their bank

; The level names: this layout keeps 19 tiles a name, a translevel's at
; its number times 19 into a table whose 24-bit pointer is at $03BB57 (in
; free space), where the game composed each from parts (LevelNames,
; CODE_049D07). At the two places the game puts a level's name in the
; stripe image (the overworld's load, and a level tile reached), Kobo's
; code puts the table's, and the game goes on past the call.
!names_at = $03BB57
!stripe_size = $7F837B
!stripe = $7F837D

org $048E81
    JSL level_name
    BRA +
    db $EA,$EA,$EA,$EA
+

org $049549
    JSL level_name
    BRA +
    db $EA,$EA,$EA,$EA
+

; The level a translevel enters (CODE_05D8A2): the game takes the number's
; high byte from the player's submap, 1 on any submap; this layout from
; the translevel, 1 from $25 on, on whichever map, but when the overworld
; override ($0109, the intro level's) names the level, which keeps the
; game's rule. In place of the game's BEQ and LDA #$01, with the submap in
; A (8-bit); the game stores what is left in A as the high byte.
!overworld_override = $0109|!addr
!translevel = $13BF|!addr

org $05D8B1
    JSL level_bank

; Each translevel's settings at a new game (OWLevelTileSettings): the game
; opens the directions of 8 level tiles (InitLevelTileMovementData); this
; layout gives every translevel its whole byte, from a table of $60 at
; $05DDA0, which the build writes. In place of the game's loop's step,
; after its first entry, which the table's copy then covers.
!level_flags = $05DDA0
!save_buffer = $1F49|!addr
!level_settings = $1EA2|!addr
!layer1_tile = $13C1|!addr
; The player whose turn it is, times 4 (PlayerTurnOW), the players' places
; in 16x16 tiles (OWPlayerXPosPtr, OWPlayerYPosPtr), and their submaps.
!player_turn = $0DD6|!addr
!player_x = $1F1F|!addr
!player_y = $1F21|!addr
!player_submaps = $1F11|!addr
!save_tiles = $03BA26

org $009F19
    JSL init_flags

; The save prompt when a level is passed (the overworld's process 2,
; CODE_048F87): the game's for the 8 level tiles of its list
; (DATA_048F7F), and this layout's for a translevel whose settings have
; bit 4 set.
org $048F94
save_prompt:
org $049003
no_save_prompt:
org $048F8A
    JSL save_check
    BCS save_prompt
    JMP no_save_prompt
    NOP

; Whether a level tile can be entered (OWPU_NotOnPipe): the game's tiles
; from $81 on cannot, nor, in this layout, a passed level whose settings
; have bit 5 set. Leaves carry set for the game's branch after it.
org $049199
    JSL entry_check

; The game's loop through its list, from its first branch on: Kobo's code
; makes the event's tiles, and the game goes on at $04E9FC.
org $04E9F7
    JSL extras
    NOP

; The layer 2 load's loop through the events, from its INC on: Kobo's code
; makes the event's further tiles and steps to the next, and the game's
; branch goes back to its JSR for that one's entries.
org $04DCA2
layer2_entries:
org $04DCA5
    JSL load_step
    BNE layer2_entries
    RTS
    NOP
    NOP

freecode
; Entered as CODE_04D7F2 left the scan, A 8-bit, X and Y 16-bit, with
; the translevels' destination's low word in X. Leaves A 8-bit and X and Y
; 16-bit for the game's loop through the events at $04D84F.
load_tables:
    PHB
    PHK
    PLB
    REP #$20
    LDA.l $04D803
    STA $8A
    STX $00
    SEP #$20
    LDA.l $04D808
    STA $8C
    LDA.b #(!map16_low+$800)>>16
    STA $02
    JSL unpack
    REP #$20
    LDA.l $04D822
    STA $8A
    LDA.w #!map16_high&$FFFF
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
    PEA.w (!rtl_00-1)&$FFFF
    JML !decompress
.back:
    PLP
    PLB
    RTL

; The events' further tiles, entered from the game's list's place at
; $04E9F7 with the event in A, A and the index registers 8-bit; leaves them
; 8-bit. At the end of an event in play (the event process's last state)
; it makes all of them; at the overworld's load (the process at 0) only
; those in layer 1, which the load has by then: those in layer 2 the
; layer 2 load made, each passed event's after its own entries
; (load_step), as a saved game's must be for an event's tiles to cover
; another's in the order they were made.
;
; Each table has a pointer below, at an offset from the start the build
; and Lunar Magic's layout know: the ranges (each event's first entry, and
; the end of the last, $79 words, in bytes of the word tables), each
; entry's data (a layer 1 tile with its page, or a layer 2 block's event
; tile data), its place, and its kind (bit 0 set for layer 2), a byte
; each.
extras:
    BRA .run
    fillbyte $FF
    fill $0D-2
.ranges_at:
    dl $FFFFFF
    fill $22-$10
.data_at:
    dl $FFFFFF
    fill $28-$25
.places_at:
    dl $FFFFFF
    fill $34-$2B
.kinds_at:
    dl $FFFFFF
.run:
    LDY #$03
    PHA
    LDA !event_process
    BNE +
    LDY #$01
+   PLA
    JSR make_extras
    SEP #$30
    RTL

assert .ranges_at-extras == $0D
assert .data_at-extras == $22
assert .places_at-extras == $28
assert .kinds_at-extras == $34

; The layer 2 load's loop through the events (CODE_04DC6A), from $04DCA5,
; after the game's own entries of event $0F: that event's layer 2 further
; tiles if it is passed, then the next event, all $78 of them. Returns
; with the comparison for the loop's branch.
load_step:
    SEP #$30
    LDA $0F
    AND #$07
    TAX
    LDA $0F
    LSR A
    LSR A
    LSR A
    TAY
    LDA !events_passed,Y
    AND.l .bits,X
    BEQ +
    LDA $0F
    LDY #$02
    JSR make_extras
    SEP #$30
+   INC $0F
    LDA $0F
    CMP #$78
    RTL

.bits:
    db $80,$40,$20,$10,$08,$04,$02,$01

; Makes the further tiles of event A (8-bit) whose kinds Y (8-bit) has:
; bit 0 those in layer 1, bit 1 those in layer 2.
make_extras:
    REP #$30
    AND #$00FF
    ASL A
    PHY
    TAY
    LDA.l extras_ranges_at
    STA $00
    LDA.l extras_ranges_at+1
    STA $01
    INY
    INY
    LDA [$00],Y
    PHA
    DEY
    DEY
    LDA [$00],Y
.next:
    ; A this entry, in bytes; the end on the stack, the kinds under it.
    CMP 1,S
    BCS .done
    PHA
    LSR A
    TAY
    LDA.l extras_kinds_at
    STA $00
    LDA.l extras_kinds_at+1
    STA $01
    LDA [$00],Y
    LSR A
    ; Carry set for layer 2. The entry and its place and data.
    LDA 1,S
    TAY
    LDA.l extras_places_at
    STA $00
    LDA.l extras_places_at+1
    STA $01
    LDA [$00],Y
    TAX
    LDA.l extras_data_at
    STA $00
    LDA.l extras_data_at+1
    STA $01
    LDA 5,S
    BCS .layer2
    AND #$0001
    BEQ .made
    LDA [$00],Y
    SEP #$20
    STA.l !map16_low,X
    XBA
    STA.l !map16_high,X
    BRA .made
.layer2:
    AND #$0002
    BEQ .made
    LDA [$00],Y
    TAY
    STX $04
    PHK
    PER .made-1
    PEA.w (!rtl_04-1)&$FFFF
    JML !make_block
.made:
    REP #$30
    PLA
    INC A
    INC A
    BRA .next
.done:
    PLA
    PLY
    RTS

; A level's name in the stripe image, as CODE_049D07 puts one: 19 tiles to
; layer 3 at VRAM $508B, each with properties $39. Entered with the
; translevel in A, A and the index registers 16-bit, as the game's code
; calls CODE_049D07; leaves them 16-bit, and $00-$03 changed, as that
; does. The data bank is kept.
level_name:
    PHB
    PHA
    SEP #$20
    LDA.l !names_at+2
    PHA
    PLB
    REP #$20
    PLA
    AND #$00FF
    STA $00
    ASL A
    ADC $00
    STA $02
    LDA $00
    ASL A
    ASL A
    ASL A
    ASL A
    CLC
    ADC $02
    CLC
    ADC.l !names_at
    TAY
    LDA.l !stripe_size
    TAX
    LDA #$8B50
    STA.l !stripe,X
    LDA #$2500
    STA.l !stripe+2,X
    LDA.w #19
    STA $00
.tile:
    SEP #$20
    LDA $0000,Y
    STA.l !stripe+4,X
    LDA #$39
    STA.l !stripe+5,X
    REP #$20
    INX
    INX
    INY
    DEC $00
    BNE .tile
    SEP #$20
    LDA #$FF
    STA.l !stripe+4,X
    REP #$20
    TXA
    CLC
    ADC #$0004
    STA.l !stripe_size
    PLB
    RTL

; The high byte of the level number a translevel enters: A the player's
; submap (8-bit) in, the byte out.
level_bank:
    PHA
    LDA !overworld_override
    BEQ .by_translevel
    PLA
    BEQ +
    LDA #$01
+   RTL
.by_translevel:
    PLA
    LDA !translevel
    CMP #$25
    LDA #$00
    ROL A
    RTL

; A new game's settings for every translevel, from the table, into the save
; buffer. A and the index registers 8-bit, as the game's loop leaves them.
init_flags:
    LDX #$5F
-   LDA.l !level_flags,X
    STA !save_buffer,X
    DEX
    BPL -
    RTL

; Carry set when the level the player has passed (TranslevelNo) brings up
; the save prompt: its settings' bit 4, or a level tile of the game's list
; unless !save_tiles is 0, Lunar Magic's Extra Option for them off, where
; its editor and transfer read the option (docs/lunar-magic-install.md,
; "The overworld"); the game has $FF there. A and the index registers
; 8-bit.
save_check:
    LDX !translevel
    LDA !level_settings,X
    AND #$10
    BNE .yes
    LDA !save_tiles
    BEQ .no
    LDX #$07
-   LDA !layer1_tile
    CMP.l $048F7F,X
    BEQ .yes
    DEX
    BPL -
.no:
    CLC
    RTL
.yes:
    SEC
    RTL

; Carry set when the level tile in A (8-bit) cannot be entered: from $81
; on, as the game has it, or a passed level whose settings have bit 5 set,
; the level by the translevel at the player's place (OW_TilePos_Calc's
; index, on the submaps' map past $400). A and the index registers 8-bit,
; A the tile again.
entry_check:
    CMP #$81
    BCS .done
    PHX
    PHY
    REP #$30
    LDA !player_turn
    AND #$00FF
    TAX
    ; The index: x and y's low nibbles, then their bit 4 as $100 and $200.
    LDA !player_y,X
    AND #$000F
    ASL A
    ASL A
    ASL A
    ASL A
    PHA
    LDA !player_y,X
    AND #$0010
    ASL A
    ASL A
    ASL A
    ASL A
    ASL A
    ORA 1,S
    STA 1,S
    LDA !player_x,X
    AND #$0010
    ASL A
    ASL A
    ASL A
    ASL A
    ORA 1,S
    STA 1,S
    LDA !player_x,X
    AND #$000F
    ORA 1,S
    STA 1,S
    TXA
    LSR A
    LSR A
    TAX
    LDA !player_submaps,X
    AND #$00FF
    BEQ +
    LDA 1,S
    ORA #$0400
    STA 1,S
+   PLX
    SEP #$20
    LDA.l !map16_low+$800,X
    SEP #$10
    TAX
    LDA !level_settings,X
    AND #$A0
    CMP #$A0
    PLY
    PLX
    LDA !layer1_tile
.done:
    RTL
