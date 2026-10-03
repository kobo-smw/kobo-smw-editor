; Lunar Magic's placed objects 22, 23, 27, and 29 (direct Map16 tiles), its
; music and time limit bypasses (26, 28), and its user object (2D).
;
; Kobo's own code, written from the community's documentation of the
; objects' formats (smwspeedruns' level data format, Lunar Magic's help on
; conditional Direct Map16), the vanilla object loader it plugs into, and
; the tiles a Lunar Magic-saved ROM's load leaves in the level for each
; form (examples/lm_objects.rs), never from Lunar Magic's code.
;
; Each object set's dispatch (the ExecutePtrLong tables ten bytes into
; OBJTS*) runs an object's routine with A, X, and Y 8-bit, the object's
; first three bytes read ($59 the third, $5A its number), $65 at its fourth
; byte, and the tile cursor set ($57, and the Map16 pointers $6B and $6E).
; The routines return with RTS to bank $0D, so the dispatch points at stubs
; there, which call Kobo's code; it places tiles through the game's own
; helpers, which follow the level's layout, reached through gates in bank
; $0D too. Bank $0D from $0DFF67 is free in every ROM of the corpus.
;
; Lunar Magic's save puts its own object code in place of this (its check
; for that piece is on its own code), and reads the same level data. Its
; code takes 26, 28, and 2D before the dispatch; Kobo's entries for them
; are used only until then.

incsrc "memory.asm"

!OBJ_SIZE = $59             ; the object's third byte
!OBJ_NUMBER = $5A
!DATA = $65                 ; the level data pointer, at the next byte
!POS = $57                  ; the tile cursor within the screen
!HIGH = $6E                 ; the high byte plane's pointer
!FLAGS = $7FC060            ; conditional Direct Map16 flags, bit n%8 of byte n/8

; The dispatch entries, each object's number less one: 22 and 23, 27 and 29,
; 26, 28, and 2D, in each of the five object sets.
macro set(table)
    org <table>+($21*3) : dl page_stub
    org <table>+($22*3) : dl page_stub
    org <table>+($26*3) : dl block_stub
    org <table>+($28*3) : dl block_stub
    org <table>+($25*3) : dl music_stub
    org <table>+($27*3) : dl time_stub
    org <table>+($2C*3) : dl user_stub
endmacro
%set($0DA455)               ; normal
%set($0DC19A)               ; castle
%set($0DCD9A)               ; rope
%set($0DD99A)               ; underground
%set($0DE89A)               ; ghost house

org $0DFF70
page_stub:
    autoclean JSL page_object
    RTS
block_stub:
    JSL block_object
    RTS
music_stub:                 ; 26: the level's music is the third byte less one
    LDA !OBJ_SIZE
    DEC A
    STA $0DDA|!addr
    RTS
user_stub:                  ; 2D: five bytes, for the user's own code
    JSL skip_two
    RTS
time_stub:                  ; 28: the time limit
    JSL time_limit
    RTS
; Gates to the game's helpers, for code outside bank $0D.
put_tile:                   ; A = low byte at [$6B],Y, then one tile right
    JSR $A95B
    RTL
save_row:                   ; remember where a row starts
    JSR $A6B1
    RTL
next_row:                   ; back to the row's start, then one tile down
    JSR $A6BA
    JSR $A97D
    RTL

freecode

; Scratch, free while an object runs: the game's helpers use $04-$05.
!WIDTH = $00                ; columns less one
!HEIGHT = $01               ; rows left, less one
!COLUMNS = $02              ; columns left in this row, less one
!SEL_W = $06                ; the selection's columns less one
!SEL_H = $07                ; its rows less one
!SEL_X = $08
!SEL_Y = $09
!ROW_LOW = $0A              ; the tile number of the selection row's start
!ROW_HIGH = $0B
!BASE = $0C                 ; the selection's top left tile, 16-bit
!ADD = $0E                  ; added to every tile's high byte
!FLAG = $0F                 ; a conditional flag's byte

; Object 28, the time limit: N10-TTTT 1000OOOO R---HHHH, its tens, ones,
; and hundreds, each a digit 0 to F. With R it sets the timer whenever the
; level loads; without, only on an entry from the overworld (no sublevel
; yet), as the header's setting does, which it replaces. The bits between R
; and the hundreds are ignored (observed, examples/lm_objects.rs timer).
time_limit:
    LDA !OBJ_SIZE
    BMI .set
    LDA $141A|!addr
    BNE .done
.set:
    REP #$20                ; the object's first byte, three before !DATA
    LDA !DATA
    SEC
    SBC #$0003
    STA $00
    SEP #$20
    LDA !DATA+2
    STA $02
    LDA [$00]
    AND #$0F
    STA $0F32|!addr
    LDY #$01
    LDA [$00],y
    AND #$0F
    STA $0F33|!addr
    LDA !OBJ_SIZE
    AND #$0F
    STA $0F31|!addr
.done:
    RTL

; Objects 22 and 23: a rectangle of one tile from page 0 or 1, the tile's
; low byte in the fourth byte, the size in the third (hhhhwwww, less one).
page_object:
    LDA [!DATA]
    STA !BASE
    LDA !OBJ_NUMBER
    AND #$01
    STA !BASE+1
    LDA #$01
    JSR skip
    LDA !OBJ_SIZE
    JSR rectangle_size
    STZ !SEL_W
    STZ !SEL_H
    STZ !ADD
    JMP draw

; Objects 27 (pages 00-3F) and 29 (40-7F): from the fourth byte, 11BBBBBB
; bbbbbbbb, the form and the top left tile of a selection of Map16 tiles
; laid out 16 to a row, then as the form says.
block_object:
    LDY #$01
    LDA [!DATA],y
    STA !BASE
    LDA [!DATA]
    AND #$3F
    LDX !OBJ_NUMBER
    CPX #$29
    BNE +
    ORA #$40
+   STA !BASE+1
    STZ !ADD
    LDA [!DATA]
    ROL A
    ROL A
    ROL A
    AND #$03
    ASL A
    TAX
    JMP (.forms,x)
.forms:
    dw .single, .block, .tiled, .wide

; A rectangle of one tile.
.single:
    LDA !OBJ_SIZE
    JSR rectangle_size
    STZ !SEL_W
    STZ !SEL_H
    LDA #$02
    JSR skip
    JMP draw

; The selection itself, its size in the third byte.
.block:
    LDA !OBJ_SIZE
    JSR rectangle_size
    LDA !WIDTH
    STA !SEL_W
    LDA !HEIGHT
    STA !SEL_H
    LDA #$02
    JSR skip
    JMP draw

; A rectangle tiled with the selection, whose size is the sixth byte.
.tiled:
    LDA !OBJ_SIZE
    JSR rectangle_size
    LDY #$02
    LDA [!DATA],y
    JSR selection_size
    LDA #$03
    JSR skip
    JMP draw

; Tiled across screens: 7-bit width in the third byte, the selection in
; the sixth, 8-bit height in the seventh; with the third byte's top bit, an
; eighth byte ACCCCCCC names a conditional flag.
.wide:
    LDA !OBJ_SIZE
    AND #$7F
    STA !WIDTH
    LDY #$03
    LDA [!DATA],y
    STA !HEIGHT
    LDY #$02
    LDA [!DATA],y
    JSR selection_size
    LDA !OBJ_SIZE
    BMI .conditional
    LDA #$04
    JSR skip
    JMP draw

; Flag C clear: nothing is drawn, unless A, which then draws the tiles as
; they are and adds $100 to them when C is set.
.conditional:
    LDY #$04
    LDA [!DATA],y
    PHA
    LDA #$05
    JSR skip
    PLA
    PHA
    AND #$7F
    JSR flag_set
    STA !ADD
    PLA
    BPL .shown_if_set
    JMP draw                ; "always show": draw, with $100 added if set
.shown_if_set:
    LDA !ADD
    BEQ .hidden
    STZ !ADD
    JMP draw
.hidden:
    RTL

; A = hhhhwwww: the rectangle's columns and rows, less one.
rectangle_size:
    PHA
    AND #$0F
    STA !WIDTH
    PLA
    LSR A
    LSR A
    LSR A
    LSR A
    STA !HEIGHT
    RTS

; A = hhhhwwww: the selection's columns and rows, less one.
selection_size:
    PHA
    AND #$0F
    STA !SEL_W
    PLA
    LSR A
    LSR A
    LSR A
    LSR A
    STA !SEL_H
    RTS

skip_two:
    LDA #$02
    JSR skip
    RTL

; Moves the level data pointer past A more bytes.
skip:
    CLC
    ADC !DATA
    STA !DATA
    LDA !DATA+1
    ADC #$00
    STA !DATA+1
    RTS

; A = flag number, 0-127. Returns A = 1 if it is set, else 0. The flags
; are in work RAM, which under SA-1 Pack, where this runs on the SA-1, the
; S-CPU reads (as a Lunar Magic-saved SA-1 ROM reads them there too).
flag_set:
    PHA
    LSR A
    LSR A
    LSR A
    STA !FLAG
    %call_scpu(read_flags)
    PLA
    AND #$07
    TAY
    LDA !FLAG
-   DEY
    BMI +
    LSR A
    BRA -
+   AND #$01
    RTS

; On the S-CPU: the flags' byte !FLAG, into !FLAG.
read_flags:
    LDX !FLAG
    LDA.l !FLAGS,x
    STA !FLAG
    RTL

; A = the low byte, written at the cursor, which then moves one tile right:
; by the game's step in a horizontal level, and in a vertical one past a
; half's last column into the screen's right half, $100 bytes on, where
; the game's step goes $1B0 bytes on.
put:
    PHA
    JSR vertical
    BNE +
    PLA
    JSL put_tile
    RTS
+   PLA
    STA [$6B],y
    INY
    TYA
    AND #$0F
    BNE +
    INC $6C
    INC $6F
    LDA !POS
    AND #$F0
    TAY
+   RTS

; Z clear if the layer being loaded is vertical.
vertical:
    LDA $5B                 ; bit 0 layer 1's vertical flag, bit 1 layer 2's
    LDX $1933|!addr
    BEQ +
    LSR A
+   AND #$01
    RTS

; After the game's step down: in a vertical level, a step past a screen's
; last row goes on to the next screen, $200 bytes on, where the game's step
; carries into the screen's right half, $100 bytes on.
vertical_down:
    LDA !POS
    AND #$F0
    BNE +                   ; still in the same screen
    JSR vertical
    BEQ +
    INC $6C
    INC $6F
    INC $05
+   RTS

; Fills the rectangle from the cursor right and down, row by row, with the
; selection repeated: the tile at selection column x of row y is the base
; plus y * 16, with x added to its low byte alone.
draw:
    STZ !SEL_Y
    LDY !POS
    JSL save_row
.row:
    LDA !SEL_Y              ; the selection row's first tile
    ASL A
    ASL A
    ASL A
    ASL A
    CLC
    ADC !BASE
    STA !ROW_LOW
    LDA !BASE+1
    ADC #$00
    CLC
    ADC !ADD
    STA !ROW_HIGH
    STZ !SEL_X
    LDA !WIDTH
    STA !COLUMNS
.column:
    LDA !ROW_HIGH
    STA [!HIGH],y
    LDA !ROW_LOW
    CLC
    ADC !SEL_X
    JSR put
    LDA !SEL_X
    CMP !SEL_W
    INC !SEL_X
    BCC +
    STZ !SEL_X
+   DEC !COLUMNS
    BPL .column
    JSL next_row
    JSR vertical_down
    LDA !SEL_Y
    CMP !SEL_H
    INC !SEL_Y
    BCC +
    STZ !SEL_Y
+   DEC !HEIGHT
    BPL .row
    RTL
