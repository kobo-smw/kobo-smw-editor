; Lunar Magic's taller levels ("ExLevel", 3.00): a per-level size byte that
; trades a horizontal level's screens for height, and the code that lets
; the game work with levels of any of its 32 heights.
;
; Kobo's own code, written from the vanilla code it hooks (SMWDisX), the
; size table's format the community documents (docs/lunar-magic.md), and
; what a Lunar Magic-saved ROM leaves in memory for each size
; (examples/exlevel_probe.rs; docs/lunar-magic-install.md, "Taller
; levels"), never from Lunar Magic's code.
;
; The size byte is TB0MMMMM: MMMMM picks a height and how many screens fit
; the level's tile planes (the table below), T that a level with layer 2
; objects splits those screens between its layers, B that the camera
; stops a pixel higher at the level's bottom. A vertical level takes no
; size.
;
; The game finds a horizontal level's screens through per-mode tables of
; screen pointers (LoadBlkPtrs) and its blocks through tables of screen
; offsets, all for 27 rows. Here those point at tables in RAM that the
; entrance code builds for the level's height, as Lunar Magic's layout has
; them: $0BF6 32 3-byte pointers into $7EC800, $0C56 the same into
; $7FC800 (SA-1 Pack's $40C800 and $41C800), $0CB6 and $0CD6 their low and high bytes; the level's height in
; $13D7, less a row in $1936, and its size byte in $0BF5.
;
; Lunar Magic counts this piece as installed when $05DA8A is a JSL, and
; finds the size table $240 bytes before the code it calls: a save then
; keeps Kobo's code and its table (docs/lunar-magic-install.md).

incsrc "memory.asm"

!size = !addr|$0BF5             ; the level's size byte
!height = !addr|$13D7           ; its height in pixels
!last_row = !addr|$1936         ; the height less a row
!low = !addr|$0BF6              ; 32 screens: 3-byte pointers into $7EC800
!high = !addr|$0C56             ; and into $7FC800
!offset_low = !addr|$0CB6       ; the pointers' low bytes
!offset_high = !addr|$0CD6      ; and high bytes
!tTT = !addr|$0BF4              ; the level's sprite spawn settings
!vertical_part = !addr|$0BDD    ; the object loader's rows, in 32s, from
                                ; the last screen jump (Kobo's own)

; Scratch while the tables are built, in the direct page, which the
; routines here put back as they found it: nothing is left in RAM that a
; Lunar Magic-saved ROM does not leave (a level with layer 2 objects past
; its screens reads its tiles from low RAM).
!stride = $00                   ; 2: bytes a screen
!split = $02                    ; 2: the screen layer 2's pointers start at
!pointer = $04                  ; 2: the screen's address, 0 once past $FFFF
!screen = $06                   ; 1: the screen the next entry is for

; ------------------------------------------------------------------------
; Hooks

; The primary header load, after the level's own settings, before the
; vertical level's own: LDA $5B : AND #$01, A 8-bit, X and Y 16-bit.
org $05D9A1
    autoclean JSL setup

; A castle's "No Yoshi" intro, which loads a level of the game's own over
; the one entered: LDA [$CE] : AND #$3F. Lunar Magic's check.
org $05DA8A
    JSL castle_intro

; The level's sprite memory setting without the new sprite system's bit,
; which goes to !size's bit 5 (AND #$3F of the sprite header's first byte).
org $05D8FC
    db $1F

; Choc Island 2's rooms, which load the game's own levels: LDA [$CE] :
; AND #$7F.
org $05DB5F
    JSL choc_rooms

; The credits' backgrounds, drawn through the level loader: STA $65 : SEP
; #$30, A 16-bit.
org $0C9436
    JSL credits

; The screen pointer tables of the level modes the game gives a horizontal
; level's 27 rows (their entries lead to DATA_00BAD8, DATA_00BB08,
; DATA_00BC40, DATA_00BC70) point at the tables in RAM.
macro mode(m)
    org $00BDA8+(<m>*2) : dw !low
    org $00BDE8+(<m>*2) : dw !low+$30
    org $00BE28+(<m>*2) : dw !high
    org $00BE68+(<m>*2) : dw !high+$30
endmacro
%mode($00)
%mode($01)
%mode($02)
%mode($0C)
%mode($0E)
%mode($0F)
%mode($11)
%mode($1E)
%mode($1F)

; Block lookups: the screen offsets for layer 1 (DATA_00BA60, DATA_00BA9C)
; and layer 2 (16 screens on, DATA_00BA70, DATA_00BAAC) from RAM.
macro offsets(at, from, to)
    org <at>+1 : dw <to>
endmacro
macro lookup(a, b, c, d)
    %offsets(<a>, $BA60, !offset_low)
    %offsets(<b>, $BA70, !offset_low+$10)
    %offsets(<c>, $BA9C, !offset_high)
    %offsets(<d>, $BAAC, !offset_high+$10)
endmacro
%offsets($00F492, $BA60, !offset_low)      ; the player's, layer 1
%offsets($00F49A, $BA9C, !offset_high)
%offsets($00F50D, $BA70, !offset_low+$10)  ; and layer 2
%offsets($00F515, $BAAC, !offset_high+$10)
%lookup($019500, $019509, $019512, $01951B)  ; sprites'
%offsets($01D97B, $BA60, !offset_low)      ; the rope's
%offsets($01D981, $BA9C, !offset_high)
%lookup($0292F9, $029301, $02930A, $029312)  ; bounce blocks'
%lookup($0295EC, $0295F4, $0295FD, $029605)  ; the cape's
%lookup($02A6BA, $02A6C2, $02A6CB, $02A6D3)  ; fireballs'
%lookup($02BA71, $02BA79, $02BA82, $02BA8A)  ; Yoshi's berries
%lookup($02D18C, $02D194, $02D19D, $02D1A5)  ; a sprite's block (CODE_02D149)

; Out of the level below its height: CMP #$01B0 in the player's layer 1 and
; layer 2 block lookups and a sprite's, ADC #$01B0 for the second screen of
; the Reznor platforms' tiles.
org $00F478 : CMP !height
org $00F4F3 : CMP !height
org $0194D6 : CMP !height
org $03D793 : ADC !height

; A point of the player's out of the level, beyond its height, its screens,
; or a vertical level's two columns (CODE_00F4A0 and CODE_00F4E7, every
; out-of-bounds branch of the player's block lookups): the game returns
; tile $25 and leaves $1693 as the last block in the level left it; a Lunar
; Magic-saved ROM also leaves $25 there (RAM, by calls of CODE_00F465 at
; points in and out of the level, docs/lunar-magic-install.md, "Taller
; levels"). X and Y 8-bit.
org $00F4A0
    BRA out_of_bounds_stub
    NOP
org $00F4E7
out_of_bounds_stub:
    JML out_of_bounds

; The high bytes of the lookups' Y below the level: SBC #$00 : CMP #$02,
; and so on, in each lookup that checks it.
org $0292D7 : JSL below_sbc     ; bounce blocks
org $0295BE : JSL below_adc     ; the cape
org $02BA4E : JSL below_yoshi   ; Yoshi's berries
org $02D158 : JSL below_adc     ; a sprite's block

; A tile generated in play: its row within the screen (AND #$01F0) from any
; height, in GenerateTile's routines and the game's column and row builds.
org $00C07B+2 : db $3F
org $00C0C8+2 : db $3F
org $00C1B3+2 : db $3F
org $00C3D5+2 : db $3F
org $058A18+2 : db $3F
org $058AF4+2 : db $3F
org $058BE6+2 : db $3F
org $058CD9+2 : db $3F

; And whether the tile is in the level at all: CPY #$0200 : BCS.
org $00BEEC
    JML tile_bound

; The object loader's steps to the next screen right (AdvanceRightOneTile)
; and the diagonal steps (CODE_0DA9D6, CODE_0DA9EF): a screen's bytes are
; the level's height, or $100 in a vertical level, where the step right
; goes into the screen's right half.
org $0DA974
advance_right_done:             ; INC $1BA1
org $0DA963
    JSL step_right
    BRA advance_right_done
org $0DA9D6
    JSL step_left
    RTS
org $0DA9EF
    JSL step_right_diagonal
    RTS

; The ground shaking (CODE_00A2A9, from STZ $1888 to the frame's other
; routines): one pixel up at the level's top, and with B two pixels higher
; than the game's, as Lunar Magic's layout has it.
org $00A2D5
shake_end:                      ; JSR UpdateStatusBar
org $00A2AF
    JSL shake
    BRA shake_end

; Screen jumps with a vertical part, in the format the community documents
; for Lunar Magic 3: extended object 01's second byte's low nibble, and
; extended object 03 (the two parts the other way round, for heights past
; 16 of them), in units of 32 rows. The extended objects' dispatch
; (CODE_0DA106) runs them with RTS to bank $0D: stubs there, past
; objects.asm's. Each object's Map16 pointers (LoadLevelData, after the
; screen's pointers are read) move down by the vertical part; each layer's
; objects start with none.
org $0DA10F+(1*3) : dl screen_jump_stub
org $0DA10F+(3*3) : dl tall_jump_stub
org $0DFFC0
screen_jump_stub:
    autoclean JSL screen_jump
    RTS
tall_jump_stub:
    JSL tall_screen_jump
    RTS
assert pc() <= $0E0000

; LDA [$00],Y : STA $6B : LDA [$0D],Y : STA $6E : INY : LDA [$00],Y : STA
; $6C : LDA [$0D],Y : STA $6F, A 8-bit, Y 8-bit.
org $0586B2
object_pointers_done:           ; INY, before the pointers' banks
org $0586A1
    JSL object_pointers
    BRA object_pointers_done

; The second layer's objects: STZ $1928 : JMP LoadAgain.
org $05840C
    JML layer2_objects

; The load's end, LoadLevelDone: STZ $1933 : PLP, then RTS. Nothing of the
; vertical part is left in RAM.
org $058412
    JML objects_done

; The camera's lowest position in a horizontal level: LDA #$00C0 : JSR
; CODE_00F7F4, A 16-bit.
org $00F70D
    JML camera_bottom

; A tide's water (CODE_00A045, which the game's layer 3 setup runs for a
; tide): tile $000 in rows 16-26 of layer 2's 16 screens, the game's
; height. At a size of the level's own, Lunar Magic's load fills rows 16
; down of the screens from the one layer 2's start at (the split's, else
; 16), the first always and the rest while a screen ends within $3700
; bytes of the tile planes, which at the game's height is those 16
; (docs/lunar-magic-install.md, "Layer 3 settings").
org $00A045
    JSL tide_water
    RTS

; ------------------------------------------------------------------------

freecode

; The size table, one byte per level, and the heights of the 32 sizes. The
; castle intro's code follows at $240, where Lunar Magic's layout has it.
sizes:
    fillbyte $00 : fill $200
heights:
    dw $01B0,$01C0,$01D0,$0200,$0220,$0250,$0260,$0280
    dw $02A0,$02C0,$02F0,$0310,$0340,$0380,$03B0,$0400
    dw $0440,$04A0,$0510,$0590,$0630,$0700,$0800,$0950
    dw $0B30,$0E00,$12A0,$1C00,$3800,$0100,$00F0,$00E0

; A level of the game's own replaces the one entered: the tables for 27
; rows, and its sprite memory setting (as at $05D8F9, without the new
; sprite system's bit, which goes to !size).
castle_intro:
assert castle_intro == sizes+$240
choc_rooms:
    JSR vanilla_tables
    LDA [$CE]
    AND #$20
    TSB !size
    LDA [$CE]
    AND #$1F
    RTL

credits:
    STA $65
    SEP #$30
    JSR vanilla_tables
    RTL

; How many screens of each size fit the tile planes.
screens:
    db $20,$20,$1E,$1C,$1A,$18,$17,$16,$15,$14,$13,$12,$11,$10,$0F,$0E
    db $0D,$0C,$0B,$0A,$09,$08,$07,$06,$05,$04,$03,$02,$01,$38,$3B,$40

setup:
    PHP
    REP #$30
    PHA
    PHX
    PHY
    JSR save_scratch
    LDA $0E
    AND #$01FF
    TAX
    SEP #$20
    STZ !vertical_part          ; for the first layer's objects
    ; The sprite spawn range and smart spawning of the level's $05DE00
    ; byte (IWPXXtTT), for the sprite loader (sprites.asm): t in bit 7. A
    ; vertical level has none.
    STZ !tTT
    LDA $5B
    LSR A
    BCS +
    LDA.l $05DE00,x
    AND #$04
    ASL A
    ASL A
    ASL A
    ASL A
    ASL A
    STA !tTT
    LDA.l $05DE00,x
    AND #$03
    TSB !tTT
+   LDA $5B
    LSR A
    BCS .vertical
    LDA [$CE]
    AND #$20                    ; the new sprite system
    ORA.l sizes,x
    STA !size
    JSR tables
    BRA .done
.vertical:
    ; A vertical level's screens are the game's, 16 rows high.
    LDA [$CE]
    AND #$20
    STA !size
    REP #$20
    LDA #$01B0
    STA !stride
    LDA #$0010
    STA !split
    JSR pointers
    LDX #$001E
-   STZ !offset_low,x
    STZ !offset_high,x
    DEX
    DEX
    BPL -
    LDA #$0100
    STA !height
    LDA #$01A0
    STA !last_row
.done:
    REP #$30
    JSR restore_scratch
    PLY
    PLX
    PLA
    PLP
    LDA $5B
    AND #$01
    RTL

; Puts $00-$07 on the stack, under the caller's return address, and takes
; them back. A, X, and Y 16-bit; A is not kept.
save_scratch:
    PLX                         ; the return address
    LDA $06
    PHA
    LDA $04
    PHA
    LDA $02
    PHA
    LDA $00
    PHA
    PHX
    RTS
restore_scratch:
    PLX
    PLA
    STA $00
    PLA
    STA $02
    PLA
    STA $04
    PLA
    STA $06
    PHX
    RTS

; The tables for a level of the game's own, 27 rows high. Keeps A, X, Y
; and their sizes.
vanilla_tables:
    PHP
    REP #$30
    PHA
    PHX
    PHY
    JSR save_scratch
    SEP #$20
    STZ !size
    STZ !vertical_part
    LDA #$00
    JSR tables
    REP #$30
    JSR restore_scratch
    PLY
    PLX
    PLA
    PLP
    RTS

; A (8-bit) = the size byte: the tables for a horizontal level of that
; size. X and Y 16-bit; returns with A 16-bit.
tables:
    STA !screen                 ; the size byte, for now
    REP #$30
    AND #$001F
    ASL A
    TAX
    LDA.l heights,x
    STA !height
    STA !stride
    SEC
    SBC #$0010
    STA !last_row
    ; Layer 2's screens start 16 on, or with T at the screen whose start is
    ; nearest the middle of the tile planes, $1C00 bytes in (the lower of
    ; two as near), and no further than 16.
    LDA #$0010
    BIT !screen-1               ; T, bit 15 here
    BPL .split
    LDA !stride
    LSR A
    CLC
    ADC #$1BFF
    LDX #$0000
-   CMP !stride
    BCC +
    SBC !stride
    INX
    BRA -
+   TXA
    CMP #$0010
    BCC .split
    LDA #$0010
.split:
    STA !split
    JSR pointers
    ; Their low and high bytes, for the block lookups.
    SEP #$20
    LDX #$0000
    LDY #$0000
-   LDA !low,y
    STA !offset_low,x
    LDA !low+1,y
    STA !offset_high,x
    INY
    INY
    INY
    INX
    CPX #$0020
    BCC -
    REP #$20
    RTS

; The 32 entries of both pointer tables, from !stride and !split: 16 for
; the screens from 0, then 16 from !split. A screen past $FFFF has a
; pointer of 0. Returns with A, X, and Y 16-bit.
pointers:
    REP #$30
    LDX #$0000
    LDA #$0000
    JSR .run
    LDA !split
.run:
    ; A: the first screen of the 16. Step the address to it.
    SEP #$20
    STA !screen
    REP #$20
    LDA.w #!map16_low&$FFFF
    STA !pointer
    SEP #$20
    LDA !screen
    BEQ .entries
-   REP #$20
    LDA !pointer
    CLC
    ADC !stride
    BCC +
    STZ !pointer
    BRA .entries
+   STA !pointer
    SEP #$20
    DEC !screen
    BNE -
.entries:
    REP #$20
    LDY #$0010
-   LDA !pointer
    STA !low,x
    STA !high,x
    SEP #$20
    LDA.b #!map16_low>>16
    STA !low+2,x
    LDA.b #!map16_high>>16
    STA !high+2,x
    REP #$20
    LDA !pointer
    BEQ +
    CLC
    ADC !stride
    BCC +
    LDA #$0000
+   STA !pointer
    INX
    INX
    INX
    DEY
    BNE -
    RTS

; A tile generated in play goes into the level only within it: above the
; level's height in a horizontal layer, in the two halves of a vertical
; one. A 8-bit, Y 16-bit its Y (horizontal) or X (vertical).
; Keeps A, both its bytes (the game's code after moves the whole
; accumulator to Y).
tile_bound:
    REP #$20
    PHA
    LDA $09
    LSR A
    BCS .vertical
    TYA
    CMP.l !height
    BRA .check
.vertical:
    CPY #$0200
.check:
    PLA
    SEP #$20
    BCS .out
    JML $00BEF1
.out:
    JML $00BEBB

; The high byte of a block's Y (in A, less a borrow or plus a carry) and
; its row's (in $00, or $04 for Yoshi's): carry set when it is below the
; level. A is kept, both its bytes.
below_sbc:
    SBC #$00
    BRA below
below_adc:
    ADC #$00
below:
    XBA
    PHA
    LDA $00
below_compare:
    REP #$20
    CMP.l !height
    SEP #$20
    PLA
    XBA
    RTL
below_yoshi:
    LDA $09
    XBA
    PHA
    LDA $04
    BRA below_compare

; Extended object 01, a screen jump: the screen from the first byte, and in
; a horizontal layer the vertical part from the second's low nibble.
screen_jump:
    LDA $0A
    AND #$1F
    STA $1928|!addr
    STA $1BA1|!addr
    JSR horizontal_layer
    BCS +
    LDA $0B
    AND #$0F
    STA !vertical_part
+   RTL

; Extended object 03: the screen from the second byte's low nibble, the
; vertical part from the first's five bits. A vertical layer has its bytes'
; low nibbles swapped (CODE_0585D8), and no vertical part.
tall_screen_jump:
    JSR horizontal_layer
    BCC +
    LDA $0A
    AND #$0F
    STA $1928|!addr
    STA $1BA1|!addr
    RTL
+   LDA $0B
    AND #$0F
    STA $1928|!addr
    STA $1BA1|!addr
    LDA $0A
    AND #$1F
    STA !vertical_part
    RTL

; Carry clear when the layer being loaded scrolls horizontally.
horizontal_layer:
    LDA $1933|!addr
    BEQ +
    LDA $5B
    LSR A
    LSR A
    RTS
+   LDA $5B
    LSR A
    RTS

; An object's Map16 pointers for its screen, as the game reads them, moved
; down by the vertical part ($200 bytes for 32 rows).
object_pointers:
    LDA [$00],y
    STA $6B
    LDA [$0D],y
    STA $6E
    INY
    LDA !vertical_part
    ASL A
    CLC
    ADC [$00],y
    STA $6C
    LDA !vertical_part
    ASL A
    CLC
    ADC [$0D],y
    STA $6F
    RTL

layer2_objects:
    STZ $1928|!addr
    STZ !vertical_part
    JML $0583B8                 ; LoadAgain

objects_done:
    STZ $1933|!addr
    STZ !vertical_part
    PLP
    JML $058416                 ; RTS

; The ground shaking, as the game's code at $00A2AF has it but for two
; things: with the camera on the level's first row ($1C from 0 to $0F) the
; step up is one pixel, and elsewhere, with B, every step is two pixels
; higher. A, X, and Y 8-bit; Y is left the step's index, as there.
shake:
    STZ $1888|!addr
    STZ $1889|!addr
    LDA $1887|!addr
    BEQ .done
    DEC $1887|!addr
    AND #$03
    TAY
    PHX
    ASL A
    TAX
    REP #$20
    LDA $1C
    CMP #$0010
    BCS .rows
    LDA.l shake_top,x
    BRA .apply
.rows:
    LDA.l !size
    AND #$0040
    BEQ +
    LDA.l shake_bottom_row,x
    BRA .apply
+   LDA.l shake_offsets,x
.apply:
    PLX
    STA $1888|!addr
    CLC
    ADC $1C
    STA $1C
    SEP #$20
.done:
    RTL

shake_offsets:
    dw $FFFE,$0000,$0002,$0000
shake_top:
    dw $FFFF,$0000,$0002,$0000
shake_bottom_row:
    dw $FFFC,$FFFE,$0000,$FFFE

; The object loader's steps a screen left and right: the Map16 pointers
; ($6B, $6E, and for the diagonal steps $04) move by the level's height,
; and the screen the cursor is on ($1BA1) with them. A and X, Y 8-bit; A's
; high byte is kept, as the game's own steps keep it.
step_right:
    LDA $6B
    CLC
    ADC.l !height
    STA $6B
    STA $6E
    LDA $6C
    ADC.l !height+1
    STA $6C
    STA $6F
    RTL
step_right_diagonal:
    LDA $6B
    CLC
    ADC.l !height
    STA $6B
    STA $6E
    STA $04
    LDA $6C
    ADC.l !height+1
    STA $6C
    STA $6F
    STA $05
    INC $1BA1|!addr
    RTL
step_left:
    LDA $6B
    SEC
    SBC.l !height
    STA $6B
    STA $6E
    STA $04
    LDA $6C
    SBC.l !height+1
    STA $6C
    STA $6F
    STA $05
    DEC $1BA1|!addr
    RTL

; The player's point out of the level: the game's CODE_00F4A0 with $1693
; set too, then its LDA #$00 : RTS.
out_of_bounds:
    PLX
    LDY.b #$25
    STY $1693|!addr
    JML $00F4A3

; A 16-bit: the lowest the camera goes in a horizontal level, the level's
; height less the screen's, or with B, low enough to show the last row
; whole ($0F lower: the screen's first line shows the line below the
; camera's); then on as the game's code, which the JSR at $00F710 would
; have called.
camera_bottom:
    LDA.l !size
    AND #$0040
    BEQ +
    LDA #$000F
+   CLC
    ADC.l !height
    SEC
    SBC #$00F0
    PEA $F712
    JML $00F7F4

; The tide's water, low bytes only, as the game's: from the address of the
; first screen of layer 2's pointers. A vertical level's screens are the
; game's. Ends with A, X, and Y 8-bit and layer 2 interaction on, as
; CODE_00A045 does.
tide_water:
    REP #$30
    PEI ($00)
    PEI ($02)
    PEI ($04)
    LDA #$01B0                  ; a vertical level's: the game's
    STA $00                     ; bytes a screen
    LDA.w #(!map16_low&$FFFF)+$1B00
    STA $02                     ; the first screen's address
    LDA $5B
    LSR A
    BCS .first
    LDA !height
    STA $00
    SEP #$20                    ; entry 16 of the pointers' bytes, which
    LDA !offset_high+16         ; the block lookups keep (!low is the
    XBA                         ; sprite graphics buffer's by now)
    LDA !offset_low+16
    REP #$20
    STA $02
.first:
    LDA $00
    CMP #$0101
    BCC .done                   ; no row 16
    SBC #$0100
    LSR A
    STA $04                     ; words of rows 16 down
    LDA $02
    BEQ .done                   ; past the planes
    CLC
    ADC $00
    BCC .screen
    BNE .done                   ; the screen runs past them
.screen:
    LDA $02
    CLC
    ADC #$0100
    TAX
    LDY $04
-   LDA #$0000
    STA.l !map16_low&$FF0000,x
    INX
    INX
    DEY
    BNE -
    LDA $02
    CLC
    ADC $00
    BCS .done
    STA $02                     ; the next screen
    ADC $00
    BCS .done
    CMP #$FF01                  ; ends within $FF00
    BCC .screen
.done:
    PLA
    STA $04
    PLA
    STA $02
    PLA
    STA $00
    SEP #$30
    LDA #$80
    TSB $5B
    RTL
