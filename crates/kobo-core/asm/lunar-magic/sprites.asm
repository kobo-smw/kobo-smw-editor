; Lunar Magic's sprite loader: the new sprite system, lists past 84 sprites,
; and the sprite spawn range and smart spawning of a level's settings.
;
; Kobo's own code, written from the vanilla loader it hooks (SMWDisX), the
; list format the community documents (docs/lunar-magic.md), PIXI's source
; for the entry points its hooks return to, and what a Lunar Magic-saved
; ROM's loader does with chosen lists and camera paths, observed as memory
; effects only (examples/sprite_probe.rs; docs/lunar-magic-install.md,
; "Sprites"), never from Lunar Magic's code.
;
; A list's header has the new sprite system's bit ($20); with it, $FF
; starts a command: $00-$7F sets bits 5 and up of every following sprite's
; Y (a Y jump), $FE ends the list, $FF is a sprite whose first byte is $FF.
; A list may be longer than 256 bytes: the loop moves $CE on as it goes and
; puts it back when the loader returns. A sprite's extension bytes are
; skipped by PIXI's size table when there is one.
;
; In a horizontal level the loader spawns, each frame the game calls it,
; the sprites in the column the game's loader takes (at the screen's edge
; the camera is scrolling towards) that are within the rows of the level's
; spawn range around the camera, and those in the range's top row or its
; bottom row between the columns 3 left of the screen and 1 right of it,
; in one pass over the list, in its order. The level's tTT byte ($0BF4,
; from $05DE00 through entrance.asm) picks the range: rows from the
; camera's Y less $C0, $30, $80 to its Y plus $1A0, $110, $150 for TT 0 to
; 2 (with the camera's Y, each divided by 16); with TT 3 there is none and
; every row spawns. With t, smart spawning, each part runs only when its
; column or row is not the one it had last time. The same range, one tile
; wider above and two below, erases sprites off screen, as the game erases
; them below the level; the load leaves it in $0BF0-$0BF3, where PIXI's
; SubOffScreen reads it. A vertical level's sprites load as the game loads
; them.
;
; Lunar Magic keeps this whole group when $02AF3D is a JSL, whatever it
; calls (docs/lunar-magic-install.md); Kobo's is one of the hooks, so a
; save keeps Kobo's loader.
;
; Entry points other tools jump to, which stay entries of this loop: PIXI
; returns to $02A82E (the loop head, X the next entry's number, Y its
; offset from $CE, $CE possibly moved on), $02A968 (after its extra bits
; hook at $02A963), and replaces $02A846 (the step to the next sprite).
;
; Under SA-1 Pack the game's loop runs with 16-bit index registers, from
; its own code at $02A802, and its slot search keeps the entry's offset on
; the stack until its way out at $02A9DA (boost/sprite_load.asm). So the
; loop head takes Y of either width, a sprite goes to the game's code at
; $02A856 with 16-bit indexes, and after a spawn the two bytes SA-1 Pack
; pushed are dropped here. PIXI's SA-1 code returns to $02A82E with 16-bit
; indexes and Y unpaged, and takes $02A9D7 itself.

incsrc "memory.asm"

; Kobo's loader state, in work RAM the game does not use ($0B9B-$0BF5 is
; free past the castle cutscenes' 15 slot flags; Lunar Magic's own loader
; leaves values at $0BE7 and $0BEE-$0BF5, $0BF4 being tTT).
!saved_ce = !addr|$0BC0 ; 2: $CE as the loader was called
!page = !addr|$0BC2     ; 1: the Y jump in effect (Y bits 5 and up)
!passes = !addr|$0BC4   ; 1: the passes to run: bit 0 column, 1 top, 2 bottom
!new = !addr|$0BC5      ; 1: bit 7 the new sprite system, bit 6 a range
!top = !addr|$0BC6      ; 2: the range's top row (signed)
!bottom = !addr|$0BC8   ; 2: the range's bottom row
!xmin = !addr|$0BCA     ; 2: the leftmost column of the row passes (signed)
!xmax = !addr|$0BCC     ; 2: the rightmost column
!last_col = !addr|$0BCE ; 2: for smart spawning, the column the pass last had
!last_top = !addr|$0BD0 ; 2: and the rows
!last_bottom = !addr|$0BD2 ; 2
!col = !addr|$0BD6      ; 2: the column, in tiles (screen * 16 + X)
!last_screen = !addr|$0BD8 ; 1: the last screen a sprite can load from

!tTT = !addr|$0BF4
!keep_top = !addr|$0BF0 ; 2: the range that keeps sprites, from the camera's Y,
!keep_bottom = !addr|$0BF2 ; 2: which PIXI's SubOffScreen reads

; ------------------------------------------------------------------------
; Hooks

; The loop's start, over BMI : STA $01 : LDX #$00 : LDY #$01, with A the
; column's screen and N its sign; then, where Lunar Magic's layout has its
; loop, a way back into Kobo's code for an RTS in bank $02.
org $02A826
    autoclean JML sprite_start
    JML sprite_finish           ; $02A82A, which sprite_start returns through

; The loop head, where PIXI's hooks come back to.
org $02A82E
    JML sprite_head
    JML sprite_after            ; $02A832, from $02A9D7

; To the next sprite (INY : INY : INX : BRA), Y at the entry's second byte.
org $02A846
    JML sprite_next

; A sprite spawned: after the game's (or PIXI's) Y, LDA $00 : STA $E4,X.
org $02A968
    JSL sprite_y

; After a sprite's tables are set up: INY : LDX $02.
org $02A9D7
    JMP $A832

; The shooters' JSR LoadShooter, which ends by jumping into the loop: a
; JMP, so that the stack is left as it was (as PIXI has it).
org $02A8D8
    JMP $AB78

; All 128 load flags cleared for a new level (LDX #$3F). SA-1 Pack's own
; code there (a JML at $02ABF2) clears all 255 of its own.
if !sa1 == 0
    org $02ABF3
        db $7F
endif

; The level's first spawns (LDA #$01 : STA $55), vertical and horizontal.
org $02AC64
    JSL sprite_level
org $02ACA4
    JSL sprite_level

; The Y of the 3 platforms on chains and the 5 Eeries (AND #$01 : STA $09),
; the Boo rings' (AND #$01 : STA $0FB8,Y), and a shooter's (AND #$01 :
; STA $1793,X), with the Y jump. $02AF3D is Lunar Magic's check.
org $02AF3D
    JSL y_high_09
org $02AFA7
    JSL y_high_09
org $02AB54
    JSL y_high_boo_ring
    NOP
org $02ABD0
    JSL y_high_shooter
    NOP

; The goal tape keeps its Y's high byte whole, and its extra bits, which the
; loader leaves in $187B, are not taken from it (STA $187B,X : AND #$01 :
; STA $14D4,X); the player's contact compares the whole byte (AND #$01).
org $01C08C
    NOP #5
org $01C0E2
    db $FF

; Off screen in a horizontal level: CLC : ADC #$50 : LDA $14D4,X : ADC #$00
; : CMP #$02 before BPL to the erase, in each bank's copy.
org $01AC40
    JSL sprite_offscreen
    NOP #6
org $02D03A
    JSL sprite_offscreen
    NOP #6
org $03B86C
    JSL sprite_offscreen
    NOP #6
org $02FED6
    JSL cluster_offscreen
    NOP #6

; ------------------------------------------------------------------------

freecode

; Rows of the spawn range, from the camera's Y, by TT (0 to 2).
range_top:
    dw -$00C0, -$0030, -$0080
range_bottom:
    dw $01A0, $0110, $0150
; What keeps a sprite spawned, from the camera's Y, by TT (0 to 3).
keep_top:
    dw -$00D0, -$0040, -$0090, -$400F
keep_bottom:
    dw $01C0, $0130, $0170, $400F

; A (16-bit) divided by 16, keeping its sign.
macro asr4()
    CMP #$8000 : ROR A
    CMP #$8000 : ROR A
    CMP #$8000 : ROR A
    CMP #$8000 : ROR A
endmacro

; The start of a call, where the game's loader has put the column in $00
; and $01 (from the camera and the direction it scrolls) and A holds its
; screen, with N set for a column left of the level.
sprite_start:
    STA $01
    BMI +
    LDA #$01                    ; the column, which it has in $00-$01
    BRA ++
+   LDA #$00
++  STA !passes
    PEA $A829                   ; the loader's last RTS comes back to $02A82A
    REP #$20
    LDA $CE
    STA !saved_ce
    SEP #$20
    LDA [$CE]
    AND #$20
    ASL A
    ASL A
    STA !new                    ; bit 7: the new sprite system; bit 6: a range
    ; Smart spawning: the column only when it is not the last one.
    REP #$20
    LDA $00
    CMP !last_col
    STA !last_col
    SEP #$20
    BNE +
    BIT !tTT
    BPL +
    LDA #$01
    TRB !passes
+   LDA $5B                     ; a vertical level loads as the game does
    LSR A
    BCC +
    JMP .go
+   LDA !tTT
    AND #$03
    CMP #$03
    BNE +
    JMP .go                     ; no range: every row spawns
+   ASL A
    TAX
    REP #$20
    LDA $1C
    CLC
    ADC.l range_top,x
    %asr4()
    STA !top
    LDA $1C
    CLC
    ADC.l range_bottom,x
    %asr4()
    STA !bottom
    LDA $1A
    SEC
    SBC #$0030
    %asr4()
    STA !xmin
    LDA $1A
    CLC
    ADC #$0110
    %asr4()
    STA !xmax
    SEP #$20
    LDA #$40
    TSB !new
    LDA #$06
    TSB !passes
    BIT !tTT
    BPL .rows_seen
    REP #$20
    LDA !top
    CMP !last_top
    SEP #$20
    BNE +
    LDA #$02
    TRB !passes
+   REP #$20
    LDA !bottom
    CMP !last_bottom
    SEP #$20
    BNE .rows_seen
    LDA #$04
    TRB !passes
.rows_seen:
    REP #$20
    LDA !top
    STA !last_top
    LDA !bottom
    STA !last_bottom
    SEP #$20
.go:
    ; The column in tiles, and the last screen a sprite can load from.
    REP #$20
    LDA $00
    LSR A
    LSR A
    LSR A
    LSR A
    STA !col                    ; screen * 16 + X
    SEP #$20
    STZ !last_screen
    LDA !passes
    LSR A
    BCC +
    LDA $01
    STA !last_screen
+   LDA !passes
    AND #$06
    BEQ +
    REP #$20
    LDA !xmax
    LSR A
    LSR A
    LSR A
    LSR A
    SEP #$20
    CMP !last_screen
    BCC +
    STA !last_screen
+   LDA !passes
    BNE +
    JML $02A84B                 ; nothing to load
+   STZ !page
    LDX #$00
    LDY #$01
    ; fall through

; The loop head, X the entry's number, Y its offset from $CE. The list is
; taken once, in its order, and a sprite loads if it is in the column
; (within the range's rows) or in the range's top or bottom row (within
; its columns).
sprite_head:
    REP #$20                    ; keep Y small: move $CE on to the entry
    TYA                         ; (Y's 16 bits, from SA-1 Pack's loop or
    CLC                         ; PIXI's there, or 8)
    ADC $CE
    STA $CE
    SEP #$30
    LDY #$00
.read:
    LDA [$CE],y
    CMP #$FF
    BNE .entry
    BIT !new
    BPL .done                   ; the game's format: the end
    INY
    LDA [$CE],y
    CMP #$FF
    BEQ .entry                  ; a sprite whose first byte is $FF, Y on it
    CMP #$80
    BCS .done                   ; $FE: the end
    STA !page                   ; a Y jump
    INY
    BRA sprite_head
.done:
    JML $02A84B                 ; RTS, back through $02A82A
.entry:
    JSR entry_column
    LDA $0D
    ASL A
    ASL A
    ASL A
    ASL A
    STA $0E
    LDA $0C
    LSR A
    LSR A
    LSR A
    LSR A
    ORA $0E                     ; the entry's screen
    CMP !last_screen
    BEQ +
    BCS .done                   ; the list is in screen order
+   LDA !passes
    LSR A
    BCC .rows
    REP #$20
    LDA $0C
    CMP !col
    BNE .rows16
    SEP #$20
    BIT !new
    BVC .load                   ; no range
    JSR entry_row16
    LDA $0A
    SEC
    SBC !top
    BMI .rows16
    LDA !bottom
    SEC
    SBC $0A
    BPL .load16
.rows16:
    SEP #$20
.rows:
    LDA !passes
    AND #$06
    BEQ .skip
    REP #$20
    LDA $0C
    SEC
    SBC !xmin
    BMI .skip16
    LDA !xmax
    SEC
    SBC $0C
    BMI .skip16
    JSR entry_row16
    SEP #$20
    LDA !passes
    AND #$02
    BEQ +
    REP #$20
    LDA $0A
    CMP !top
    BEQ .load16
    SEP #$20
+   LDA !passes
    AND #$04
    BEQ .skip
    REP #$20
    LDA $0A
    CMP !bottom
    BEQ .load16
.skip16:
    SEP #$20
.skip:
    INY
    JMP sprite_next
.load16:
    SEP #$20
.load:
    ; It loads at its own column: $00 and $01 as the game's loader has them.
    LDA $0C
    ASL A
    ASL A
    ASL A
    ASL A
    STA $00
    LDA $0D
    ASL A
    ASL A
    ASL A
    ASL A
    STA $01
    LDA $0C
    LSR A
    LSR A
    LSR A
    LSR A
    ORA $01
    STA $01
    INY
if !sa1
    REP #$10                    ; SA-1 Pack's loop runs with 16-bit indexes
endif
    JML $02A856                 ; the game's: the load flag, then the sprite

; Back from the loader's last RTS: $CE as it was, and the RTS to the game.
sprite_finish:
    REP #$20
    LDA !saved_ce
    STA $CE
    SEP #$20
    JML $02A84B

; $0A-$0B: the row of the entry at Y (Y bits 5 and up from the jump).
entry_row16:
    SEP #$20
    JSR entry_row
    REP #$20
    RTS
entry_row:
    STZ $0B
    LDA [$CE],y
    LSR A
    LSR A
    LSR A
    LSR A
    STA $0A
    LDA [$CE],y
    AND #$01
    ASL A
    ASL A
    ASL A
    ASL A
    ORA $0A
    STA $0A
    LDA !page
    REP #$20
    AND #$00FF
    ASL A
    ASL A
    ASL A
    ASL A
    ASL A
    ORA $0A
    STA $0A
    SEP #$20
    RTS

; $0C-$0D: the column of the entry at Y (A 8-bit).
entry_column:
    SEP #$20
    LDA [$CE],y
    AND #$02
    LSR A
    STA $0D                     ; the screen's bit 4
    INY
    LDA [$CE],y
    LSR A
    LSR A
    LSR A
    LSR A
    STA $0C
    LDA [$CE],y
    ASL A
    ASL A
    ASL A
    ASL A
    ORA $0C
    STA $0C
    DEY
    RTS

; The step to the next sprite, Y at the entry's second byte (the game's
; LoadNextSprite, which PIXI replaces with its own).
sprite_next:
    SEP #$10                    ; from SA-1 Pack's loop, 16-bit
    DEY
    JSR entry_size
    STA $0A
    TYA
    CLC
    ADC $0A
    TAY
    INX
    JMP sprite_head

; A: the size of the entry at Y, 3 or what PIXI's size table says.
entry_size:
    LDA.l $0EF30F
    CMP #$42
    BNE .three
    PHX
    PHY
    REP #$20
    LDA.l $0EF30C
    STA $0A
    SEP #$20
    LDA.l $0EF30E
    STA $0C
    LDA [$CE],y
    LSR A
    LSR A
    AND #$03
    XBA
    INY
    INY
    LDA [$CE],y
    REP #$30
    TAY
    SEP #$20
    LDA [$0A],y
    SEP #$10
    PLY
    PLX
    CMP #$03
    BCS +
.three:
    LDA #$03
+   RTS

; After a sprite spawned, Y at its number: the goal tape keeps the entry's
; extra bits and Y bit in $187B, as the game kept them in $14D4.
sprite_after:
if !sa1
    PLA                         ; what SA-1 Pack's slot search kept of Y
    PLA                         ; (its own way out, from $02A9DA, does this)
    SEP #$10
endif
    LDA !9E,x
    CMP #$7B
    BNE +
    DEY
    DEY
    LDA [$CE],y
    AND #$0D
    STA !187B,x
    INY
    INY
+   DEY
    LDX $02
    JMP sprite_next

; A spawned sprite's Y high byte, whole, with the Y jump: the game keeps
; the extra bits there too. Y at the entry, X the slot.
sprite_y:
    LDA [$CE],y
    AND #$01
    PHA
    LDA !page
    ASL A
    ORA $01,s
    STA !14D4,x
    PLA
    LDA $00
    STA !E4,x
    RTL

; A = the entry's first byte: bit 0 and the Y jump, as a Y high byte.
y_high_09:
    AND #$01
    STA $09
    LDA !page
    ASL A
    ORA $09
    STA $09
    RTL

y_high_boo_ring:
    AND #$01
    PHA
    LDA !page
    ASL A
    ORA $01,s
    STA $0FB8|!addr,y
    PLA
    RTL

y_high_shooter:
    AND #$01
    PHA
    LDA !page
    ASL A
    ORA $01,s
    STA $1793|!addr,x
    PLA
    RTL

; A level's first spawns: the sprite memory setting without the new sprite
; system's bit, smart spawning's last column and rows forgotten, and the
; range that keeps sprites, from the camera's Y, where PIXI's SubOffScreen
; reads it ($0BF0 its top, $0BF2 its bottom, as Lunar Magic's loader leaves
; them). A vertical level has no range: TT 0's, with $0BF4 cleared.
sprite_level:
    LDA $1692|!addr
    AND #$1F
    STA $1692|!addr
    LDA $5B
    LSR A
    BCC +
    STZ !tTT
+   LDA !tTT
    AND #$03
    ASL A
    TAX
    REP #$20
    LDA.l keep_top,x
    STA !keep_top
    LDA.l keep_bottom,x
    STA !keep_bottom
    LDA #$8000
    STA !last_col
    STA !last_top
    STA !last_bottom
    SEP #$20
    LDA #$01
    STA $55
    RTL

; An off-screen sprite in a horizontal level, X its slot: returns N clear
; to erase it. Below the level it goes, as the game has it; otherwise,
; unless it processes off screen, when it is out of the range that keeps
; sprites, as PIXI's SubOffScreen has it too.
sprite_offscreen:
    LDA !14D4,x
    XBA
    LDA !D8,x
    REP #$20
    PHA
    JSR level_height
    EOR #$FFFF
    SEC
    ADC $01,s                   ; Y less the level's height
    BPL .erase
    SEP #$20
    LDA !167A,x
    AND #$04
    BNE .keep
    ; The range by TT, as the load leaves it for PIXI, but from the level's
    ; tTT itself: a level whose first spawns take another way (a boss
    ; arena's) has no range left in RAM, and keeps its sprites.
    PHX
    LDA !tTT
    AND #$03
    ASL A
    TAX
    REP #$20
    LDA $02,s
    SEC
    SBC $1C                     ; Y from the camera's
    CMP.l keep_bottom,x
    BPL .erase_x
    SEC
    SBC.l keep_top,x
    BMI .erase_x
    SEP #$20
    PLX
.keep:
    REP #$20
    PLA
    SEP #$20
    LDA #$80
    RTL
.erase_x:
    SEP #$20
    PLX
.erase:
    REP #$20
    PLA
    SEP #$20
    LDA #$00
    RTL

; A cluster sprite below the level (the game's copy of the check has no
; caller).
cluster_offscreen:
    LDA $1E2A|!addr,x
    XBA
    LDA $1E02|!addr,x
    REP #$20
    PHA
    JSR level_height
    EOR #$FFFF
    SEC
    ADC $01,s
    BPL .erase
    PLA
    SEP #$20
    LDA #$80
    RTL
.erase:
    PLA
    SEP #$20
    LDA #$00
    RTL

; A (16-bit): the level's height in pixels, which the taller levels'
; code (a JSL at $05D9A1) leaves in $13D7, else the game's.
level_height:
    LDA.l $05D9A1
    AND #$00FF
    CMP #$0022
    BNE +
    LDA $13D7|!addr
    RTS
+   LDA #$01B0
    RTS
