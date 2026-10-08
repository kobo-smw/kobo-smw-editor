; Lunar Magic's VRAM layout: layer 1's and layer 2's tilemaps 64x32 at $3000
; and $3800, which leaves $2000-$2FFF for the BG2 and BG3 graphics slots,
; with the tilemaps kept up to date as the camera moves.
;
; Kobo's own code, written from the vanilla code it hooks and the VRAM a
; Lunar Magic-saved ROM's levels leave frame by frame (docs/lunar-magic-
; install.md, "Graphics"), never from Lunar Magic's code.
;
; A 64x32 tilemap holds 32 Map16 columns and 16 rows, wrapping both ways:
; column c at tile column 2c mod 64, row r at tile row 2r mod 32. A layer's
; camera is at Map16 column cx = X / 16 and row cy = (Y + 1) / 16 (the
; screen's first line shows the line below the vertical position), and
; columns cx to cx+16 of rows cy to cy+14 are in view. A layer that scrolls
; horizontally is loaded with columns cx to cx+16 of rows cy-1 (or 0) to
; cy+14, one whose width fits (a vertical level's layers, a background)
; with every column of those rows. Then each frame, when the camera has
; moved, the column on the side it moved to is built (rows cy to cy+14)
; and the row (columns cx to cx+16, or all of them), each unless it is the
; one built last; the NMI uploads them (frame_layer has the details). This
; is what Lunar Magic's patch leaves in VRAM frame by frame, on every vanilla
; level along several camera paths (examples/gfx_probe.rs).
;
; A tile changed in play is queued as a stripe image at the game's own
; address, in its 64x64 tilemaps at $2000 and $3000, as with Lunar Magic's
; patch; at the end of the game loop (stripe_remap) each stripe for those
; tilemaps goes to its place in these, or is left out when its row is not
; in view. Patches that read the stripe buffer expect the game's addresses
; there (the Romhack Races baserom's vram_optimize.asm, which does that
; move itself at the same hook, in Lunar Magic's place).
;
; Lunar Magic decides whether its own patch is in a ROM by $00A5A2 alone
; (a JML there), which Kobo leaves as the game has it: Lunar Magic's first
; save then installs its patch over these sites (or, for $0580D3, $05879D,
; and $00BF36, its taller levels piece does). PIXI checks for the JML at
; $00F6E4.

incsrc "memory.asm"

!ScreenMode = $5B
!LevelMode = !addr|$1925
!Tileset = !addr|$1931
!LayerProcessing = !addr|$1933
!LoadIndex = !addr|$1928
!ShakeTimer = !addr|$1887
!ShakeOffset = !addr|$1888       ; what the NMI adds to layer 1's vertical scroll
!Size = !addr|$0BF5             ; the level's size byte

; What this code keeps between frames, in the RAM Lunar Magic's patch uses
; for its own. Words, one for each layer, indexed by 0 or 2; the bytes
; named after a word share it, as its high byte.
!LastX = !addr|$0695            ; the camera's position the frame before, as
!LastY = !addr|$0699            ; shown (layer 1's with the ground shaking)
!Pending = !addr|$069D          ; bit 0: a column is built, bit 1: a row
!ColumnTile = !addr|$06A1       ; the column's tile column, 0 to 63 (byte)
!RowTile = !addr|$06A2          ; the row's top tile row, 0 to 31 (byte)
!ColumnRow = !addr|$06A5        ; the tile row its first word goes to, and
                                ; in the high byte how many tile rows
!RowColumn = !addr|$06A9        ; the tile column its first word goes to (byte)
!RowCount = !addr|$06AA         ; tile columns in the row (byte)
!BuiltColumn = !addr|$06AD      ; the Map16 column built last, $FFFF for none
!BuiltRow = !addr|$06B1         ; the row built last, $FFFF for none
!BgTable = !addr|$06B5          ; a background's BG Map16 table, 3 bytes
!BgStride = !addr|$06B8         ; bytes per half of a background
!Mask = !addr|$06BA             ; ORed into a layer's tile words

; A background's tiles, 16 bits each, kept at the load from the level's
; background buffer ($7EB900 low bytes, $7EBD00 high), which the graphics
; loader may overwrite: 16 columns by 32 rows per half, row by row, the
; left half here and the right half $700 bytes on, where Lunar Magic's
; patch keeps what it needs of the background.
!BgCache = $7FBC00
!BgCacheRight = $0700

; The buffers, the game's own for these uploads ($100 bytes each): a row's
; two tile rows from +0 (up to 17 cells, or 32 when no column is built
; with it), a column's two tile columns of 15 cells from +$88, the right
; one $3C bytes after the left.
!Buffer1 = !addr|$1BE6
!Buffer2 = !addr|$1CE8
!ColumnArea = $0088
!ColumnRight = $003C

; Direct page scratch, as the game's own uploads use it.
!Column = $00                   ; a cell's Map16 column
!Row = $02                      ; its row
!Index = $04                    ; where in the buffer
!Count = $06
!Kind = $08                     ; the layer's kind, below
!Words = $0A                    ; a Map16 definition ($0A-$0C, as the lookups leave it)
!Layer = $0E                    ; 0 or 2
!Low = $6B                      ; the cell's screen, low bytes ($6B-$6D)
!High = $6E                     ; and high bytes ($6E-$70)
!CamColumn = $8A
!CamRow = $8C
!Offset = $8E

; Layer kinds, by level mode, from the game's own dispatch of its column
; and row builders (CODE_0588EC for layer 1, CODE_058955 for layer 2).
!None = $0000
!Horizontal = $0001             ; scrolls horizontally: columns and rows
!Vertical = $0002               ; a vertical level's: whole rows
!Background = $0003             ; a background: whole rows

; ---------------------------------------------------------------------------
; The level load's tilemap upload, CODE_0580BD's loop (32 passes over
; LevelLoadObject): the first pass sets the tilemaps' registers and fills
; each layer's window, in place of the loop's own column builds and
; uploads. Each pass still counts the layers' columns on ($47, $4B), and
; skips the re-pointing of the vertical pipes' Map16 by column, which the
; builds here do for what they build.
;
; The three JSLs stay JSLs: Lunar Magic's install rewrites only their
; operands.
org $0580BF
    autoclean JSL vram_load
    JSL vram_nothing
    JSL vram_nothing            ; on to REP #$30 : INC $47 : INC $4B
org $0580D3
    JMP $80FB                   ; past the pipes, to INC LevelLoadObject

; The scroll setup, CODE_05877E, re-points the vertical pipes' Map16 by the
; column its builders would build; these do not, so skip it.
org $05879D
    JMP $87E1

; Each frame, in CODE_0586F1 after the scroll setup: build what came into
; view. CODE_0586F1 ends with PLP : RTL.
org $0586F7
    JML vram_frame

; The NMI's JSL UploadOneMap16Strip.
org $008209
    JSL vram_nmi

; The camera update's SBC #$000C : STA CameraLeftBuffer, at its start,
; where the layers are still where the frame before left them: that is
; the position a frame's builds compare the camera with. PIXI checks this
; site for Lunar Magic's patch.
org $00F6E4
    JML camera_left_edge

; A tile changed in play (GenerateTile): its VRAM address in the game's
; tilemaps, and whether it is in the game's window, for a level of any
; size. The BEQ at $00BF35 goes on to the JML either way.
org $00BF36
    db $00
    JML tile_address

; The game loop's JSR RunGameMode: then the frame's stripe images go to
; these tilemaps. Lunar Magic's patch hooks the same bytes (JMP $BA56, and
; $00BA56-$00BA5C, vanilla fill); a patch that does this itself puts its
; own there (vram_optimize.asm: a JSR to $00BA56, with an autoclean JSL at
; $00BA59, which a JML there does not set off).
org $008072
    JMP $BA56
org $00BA56
    JSR $9322                   ; RunGameMode
    JML stripe_remap

; The NMI on a frame that lags (BEQ NotSpecialLevelNMI : JMP
; SpecialLevelNMI, after LDA $0D9B : LSR A): in a level that is not
; special, layer 1's and layer 2's scroll registers stay as the last frame
; set them, at the camera the tilemaps were built for, as in a Lunar
; Magic-saved ROM (docs/lunar-magic-install.md, "Graphics").
org $008275
lag_regular:                    ; LDA $0D9B, after the layers' scroll
org $00827A
lag_special:                    ; SpecialLevelNMI
org $0081E2
    JML lag_frame

freecode

vram_nothing:
    RTL

lag_frame:
    BNE +
    JML lag_regular
+   JML lag_special

; A 16-bit, the data bank $00. A is kept for the code after the hook.
camera_left_edge:
    SBC #$000C
    STA $142C|!addr
    PHA
    PHP
    REP #$30
    LDA $1A
    STA !LastX
    LDA $1C
    CLC
    ADC !ShakeOffset            ; the frame before's, cleared later in this one
    STA !LastY
    LDA $1E
    STA !LastX+2
    LDA $20
    STA !LastY+2
    PLP
    PLA
    JML $00F6EA

; ---------------------------------------------------------------------------
; The load. A, X, Y 16-bit; returns with A 8-bit, as INC LevelLoadObject
; after it wants.
vram_load:
    PHB
    PHK
    PLB
    LDA !LoadIndex
    AND #$00FF
    BNE .done
    SEP #$20
    LDA #$31                    ; layer 1: $3000, 64x32
    STA $2107
    LDA #$39                    ; layer 2: $3800, 64x32
    STA $2108
    REP #$20
    ; A background's BG Map16 table, from the level's flags.
    JSL $0EFD00
    REP #$30
    LDA !Words
    STA !BgTable
    LDA !Words+1
    STA !BgTable+1
    LDA $05
    STA !BgStride
    LDX #$0002
    JSR layer_kind
    CMP #!Background
    BNE +
    JSR cache_background
+
    LDX #$0000
    JSR load_layer
    LDX #$0002
    JSR load_layer
.done:
    PLB
    SEP #$20
    RTL

; Copies the background's tiles into !BgCache, as 16-bit numbers: rows of
; 16 of the rows a half has (27 or 32, by the stride), each half's.
cache_background:
    LDA #!BgCache
    STA $00
    SEP #$20
    LDA #!BgCache>>16
    STA $02
    REP #$20
    LDA !BgStride
    ASL A
    STA $03                     ; the end of the right half
    LDX #$0000                  ; from the buffer
    LDY #$0000                  ; into the cache
.left:
    JSR .cell
    CPX !BgStride
    BCC .left
    LDY #!BgCacheRight
.right:
    JSR .cell
    CPX $03
    BCC .right
    RTS

.cell:
    SEP #$20
    LDA.l $7EBD00,x
    XBA
    LDA.l $7EB900,x
    REP #$20
    STA [$00],y
    INY
    INY
    INX
    RTS

; X = the layer. Builds and uploads its whole window.
load_layer:
    STX !Layer
    JSR layer_kind
    STA !Kind
    BNE +
    RTS
+   JSR set_mask
    JSR camera_cell
    LDA #$FFFF
    STA !BuiltColumn,x
    STA !BuiltRow,x
    STZ !Pending,x
    ; Rows cy-1 (or 0, at the level's top) to cy+14: columns cx to cx+16
    ; of a layer that scrolls horizontally, all of any other.
    LDA #$0010
    STA !Count
    LDA !CamRow
    BEQ +
    DEC A
+   STA !Row
.row:
    LDX !Layer
    PEI (!Kind)
    PEI (!Count)
    PEI (!Row)
    LDA !Kind
    CMP #!Horizontal
    BNE +
    LDA !CamColumn
    STA !Column
    LDA #$0011
    BRA ++
+   STZ !Column
    LDA #$0020
++  JSR build_row
    LDX !Layer
    JSR upload_layer
    PLA
    INC A
    STA !Row
    PLA
    DEC A
    STA !Count
    PLA
    STA !Kind
    LDA !Count
    BNE .row
    LDA !Kind                   ; the pipes, as for the last column in view
    CMP #!Horizontal
    BNE +
    LDX !Layer
    LDA !CamColumn
    CLC
    ADC #$0010
    JSR set_pipes
+   RTS

; ---------------------------------------------------------------------------
; Each frame. A, X, Y 16-bit.
vram_frame:
    PHB
    PHK
    PLB
    LDX #$0000
    JSR frame_layer
    LDX #$0002
    JSR frame_layer
    PLB
    JML $058774                 ; PLP : RTL

; X = the layer. What came into view since the last frame, unless the NMI
; has not uploaded what was built then. A column and a row are built on the
; side the camera moved to, by its position, unless they are the ones built
; last, as Lunar Magic's patch was seen to do: a camera that turns within a
; cell builds the column or row on its other edge, and one that skips
; columns builds only the one at its edge. On the first frame after the
; load, a camera that has not moved builds the right column and the bottom
; row, as if it had come right and down.
frame_layer:
    STX !Layer
    JSR layer_kind
    STA !Kind
    BNE +
    RTS
+   LDA !Pending,x
    BEQ +
    RTS
+   JSR set_mask
    JSR camera_cell
    TXA
    ASL A
    TAY
    ; The vertical position as this frame shows it: layer 1 with the ground
    ; shaking that comes later in the frame, which the NMI adds to its
    ; scroll register.
    LDA $001C|!dp,y
    CPX #$0000
    BNE +
    JSR shake_offset
    CLC
    ADC $1C
    PHA
    INC A
    JSR shift4
    STA !CamRow
    PLA
+   PHA
    LDA !Kind
    CMP #!Horizontal
    BNE .row
    ; A column: cx+16 moving right, cx moving left.
    LDA $001A|!dp,y
    SEC
    SBC !LastX,x
    BNE +
    LDA !BuiltColumn,x          ; not moving: right, on the first frame
    CMP #$FFFF
    BNE .row
    BRA .right
+   BMI .left
.right:
    LDA !CamColumn
    CLC
    ADC #$0010
    BRA +
.left:
    LDA !CamColumn
+   CMP !BuiltColumn,x
    BEQ .row
    STA !BuiltColumn,x
    STA !Column
    LDA !CamRow
    STA !Row
    LDA #$000F
    PHY
    JSR build_column
    PLY
    LDX !Layer
.row:
    ; A row: cy+14 moving down, cy moving up.
    PLA
    SEC
    SBC !LastY,x
    BEQ .still
    BMI .up
.down:
    LDA !CamRow
    CLC
    ADC #$000E
    BRA +
.up:
    LDA !CamRow
+   CMP !BuiltRow,x
    BEQ .done
    STA !BuiltRow,x
    STA !Row
    LDA !Kind
    CMP #!Horizontal
    BNE .whole
    LDA !CamColumn              ; columns cx to cx+16
    STA !Column
    LDA #$0011
    BRA .build
.whole:
    STZ !Column
    LDA #$0020
.build:
    JSR build_row
.done:
    RTS
.still:
    LDA !BuiltRow,x
    CMP #$FFFF
    BEQ .down
    RTS

; A = this frame's ground shaking, 16-bit, as exlevel.asm's shake will set
; it after the frame's builds: from the timer before it counts down, a
; pixel less up on the level's first row, and two pixels higher with B.
; X kept.
shake_offset:
    LDA !ShakeTimer
    AND #$00FF
    BEQ .done
    AND #$0003
    ASL A
    PHX
    TAX
    LDA $1C
    CMP #$0010
    BCS +
    LDA.l shake_top,x
    BRA ++
+   LDA !Size
    AND #$0040
    BEQ +
    LDA.l shake_bottom_row,x
    BRA ++
+   LDA.l shake_offsets,x
++  PLX
.done:
    RTS

shake_offsets:
    dw $FFFE,$0000,$0002,$0000
shake_top:
    dw $FFFF,$0000,$0002,$0000
shake_bottom_row:
    dw $FFFC,$FFFE,$0000,$FFFE

; ---------------------------------------------------------------------------
; The NMI, in a level. A, X, Y 8-bit, the data bank $00.
vram_nmi:
    PHP
    REP #$30
    LDX #$0000
    JSR upload_layer
    LDX #$0002
    JSR upload_layer
    PLP
    RTL

; ---------------------------------------------------------------------------
; X = the layer. A = its kind for the level mode. Y kept.
layer_kind:
    PHY
    LDA !LevelMode
    AND #$001F
    TAY
    CPX #$0000
    BNE +
    LDA.w kinds1,y
    BRA ++
+   LDA.w kinds2,y
++  PLY
    AND #$00FF                  ; the flags from the kind
    RTS

kinds1:
    db 1,1,1,2,2,1,1,2,2,0,2,0,1,2,1,1
    db 0,1,0,0,0,0,0,0,0,0,0,0,0,0,1,1
kinds2:
    db 3,1,1,1,1,2,2,2,2,0,3,0,3,3,3,1
    db 0,3,0,0,0,0,0,0,0,0,0,0,0,0,3,1

; X = the layer. What its tile words get ORed with: layer 2's objects in
; object tileset 3 go four palettes up, as the game's builders put them.
set_mask:
    STZ !Mask
    CPX #$0000
    BEQ +
    LDA !Kind
    CMP #!Background
    BEQ +
    LDA !Tileset
    AND #$00FF
    CMP #$0003
    BNE +
    LDA #$1000
    STA !Mask
+   RTS

; X = the layer. !CamColumn and !CamRow = the Map16 cell at its camera.
; The row is the one the screen's first line shows, a line below the
; vertical position, as the PPU shows it.
camera_cell:
    TXA
    ASL A
    TAY
    LDA $001A|!dp,y
    JSR shift4
    STA !CamColumn
    LDA $001C|!dp,y
    INC A
    JSR shift4
    STA !CamRow
    RTS

; A = A / 16, rounding down, signed.
shift4:
    CMP #$8000
    ROR A
    CMP #$8000
    ROR A
    CMP #$8000
    ROR A
    CMP #$8000
    ROR A
    RTS

; X = the layer. A = its buffer.
buffer:
    CPX #$0000
    BNE +
    LDA.w #!Buffer1
    RTS
+   LDA.w #!Buffer2
    RTS

; X = the layer, !Column the Map16 column, !Row the first row, A the number
; of rows (15). Fills the buffer's two tile columns, and marks them to go up.
build_column:
    STA !Count
    ASL A
    XBA
    STA !ColumnRow,x
    LDA !Row
    ASL A
    AND #$001F
    ORA !ColumnRow,x
    STA !ColumnRow,x
    LDA !Column
    ASL A
    AND #$003F
    SEP #$20
    STA !ColumnTile,x
    REP #$20
    JSR buffer
    CLC
    ADC #!ColumnArea
    STA !Index
.cell:
    JSR cell_definition
    LDX !Index
    LDY #$0000
    LDA [!Words],y
    ORA !Mask
    STA.l $000000,x             ; top left, left column
    LDY #$0002
    LDA [!Words],y
    ORA !Mask
    STA.l $000002,x             ; bottom left
    LDY #$0004
    LDA [!Words],y
    ORA !Mask
    STA.l $000000+!ColumnRight,x ; top right, right column
    LDY #$0006
    LDA [!Words],y
    ORA !Mask
    STA.l $000002+!ColumnRight,x ; bottom right
    INX
    INX
    INX
    INX
    STX !Index
    INC !Row
    DEC !Count
    BNE .cell
    LDX !Layer
    LDA !Pending,x
    ORA #$0001
    STA !Pending,x
    LDA !Kind
    CMP #!Horizontal
    BNE +
    LDA !Column
    JSR set_pipes
+   RTS

; X = the layer, !Row the Map16 row, !Column the first column, A the number
; of columns. Fills the buffer's two tile rows, and marks them to go up.
build_row:
    STA !Count
    ASL A
    SEP #$20
    STA !RowCount,x
    REP #$20
    LDA !Row
    ASL A
    AND #$001F
    SEP #$20
    STA !RowTile,x
    REP #$20
    LDA !Column
    ASL A
    AND #$003F
    SEP #$20
    STA !RowColumn,x
    REP #$20
    JSR buffer
    STA !Index
.cell:
    JSR cell_definition
    LDX !Index
    LDY #$0000
    LDA [!Words],y
    ORA !Mask
    STA.l $000000,x             ; top left, top row
    LDY #$0004
    LDA [!Words],y
    ORA !Mask
    STA.l $000002,x             ; top right
    LDX !Layer
    LDA !RowCount,x
    AND #$00FF
    ASL A
    CLC
    ADC !Index
    TAX                         ; the bottom row
    LDY #$0002
    LDA [!Words],y
    ORA !Mask
    STA.l $000000,x             ; bottom left
    LDY #$0006
    LDA [!Words],y
    ORA !Mask
    STA.l $000002,x             ; bottom right
    LDA !Index
    CLC
    ADC #$0004
    STA !Index
    INC !Column
    DEC !Count
    BNE .cell
    LDX !Layer
    LDA !Pending,x
    ORA #$0002
    STA !Pending,x
    LDA !Kind
    CMP #!Vertical
    BNE +
    LDA !Row
    JSR set_pipes
+   RTS

; A = a Map16 column (a row in a vertical level) just built. Points the
; game's Map16 pointers for the vertical pipes, tiles $133-$13A, at the set
; of definitions for its screen, as the game's column builds leave them for
; the tile changes that read them. Layer 1's only. X kept.
set_pipes:
    CPX #$0000
    BNE .done
    LSR A
    LSR A
    LSR A
    AND #$0006
    PHX
    TAX
    LDA.l $058776,x
    LDX #$0000
-   STA $1224|!addr,x           ; Map16Pointers + $133 * 2
    CLC
    ADC #$0008
    INX
    INX
    CPX #$0010
    BCC -
    PLX
.done:
    RTS

; The Map16 definition of the layer's cell (!Column, !Row) into [!Words].
; X and Y are not kept.
cell_definition:
    LDA !Kind
    CMP #!Background
    BNE +
    JMP .background
+   CMP #!Vertical
    BEQ .vertical
    ; Scrolling horizontally: a screen is 16 columns of the level's rows.
    LDA !Row
    ASL A
    ASL A
    ASL A
    ASL A
    STA !Offset
    LDA !Column
    AND #$000F
    ORA !Offset
    STA !Offset
    LDA !Column
    BRA .screen

.vertical:
    ; A vertical level's screen is 16 rows, 16 columns and then 16 more.
    LDA !Row
    AND #$000F
    ASL A
    ASL A
    ASL A
    ASL A
    STA !Offset
    LDA !Column
    AND #$000F
    ORA !Offset
    STA !Offset
    LDA !Column
    AND #$0010
    BEQ +
    LDA !Offset
    CLC
    ADC #$0100
    STA !Offset
+   LDA !Row
.screen:
    ; A = the column or row whose 16 make the screen. Its pointers come
    ; from the level mode's tables of screen pointers (LoadBlkPtrs), as
    ; the game's builders take them.
    LSR A
    LSR A
    LSR A
    LSR A
    AND #$001F
    STA !Low
    ASL A
    CLC
    ADC !Low
    STA !Low                    ; the screen's entry, 3 bytes each
    LDA !LevelMode
    AND #$001F
    ASL A
    TAX
    LDA !Layer
    BNE .layer2
    LDA.l $00BDA8,x             ; Ptrs00BDA8: layer 1's low bytes
    CLC
    ADC !Low
    TAY
    LDA.l $00BE28,x             ; Ptrs00BE28: layer 1's high bytes
    BRA .pointers
.layer2:
    LDA.l $00BDE8,x             ; Ptrs00BDE8: layer 2's low bytes
    CLC
    ADC !Low
    TAY
    LDA.l $00BE68,x             ; Ptrs00BE68: layer 2's high bytes
.pointers:
    CLC
    ADC !Low
    TAX                         ; X: the high bytes' entry, Y: the low's
    LDA.l $000000,x
    STA !High
    LDA.l $000001,x
    STA !High+1
    TYX
    LDA.l $000000,x
    STA !Low
    LDA.l $000001,x
    STA !Low+1
    LDY !Offset
    SEP #$20
    LDA [!High],y
    XBA
    LDA [!Low],y
    REP #$20
    ; The vertical pipes, tiles $133-$13A, take one of four sets of
    ; definitions by the screen they are in (every 16 columns, or rows in
    ; a vertical level, round in four), as the game's builders set them
    ; (MAP16AppTable, $058776).
    CMP #$0133
    BCC .lookup
    CMP #$013B
    BCS .lookup
    SEC
    SBC #$0133
    ASL A
    ASL A
    ASL A
    STA !Offset
    LDA !Kind
    CMP #!Vertical
    BEQ +
    LDA !Column
    BRA ++
+   LDA !Row
++  LSR A
    LSR A
    LSR A
    AND #$0006
    TAX
    LDA.l $058776,x
    CLC
    ADC !Offset
    STA !Words
    SEP #$20
    LDA #$0D
    STA !Words+2
    REP #$20
    RTS
.lookup:
    ASL A
    JSL $06F540                 ; A = the tile * 2: its definition's address
    STA !Words
    RTS

.background:
    ; From the cache: two halves of 16 columns, rows of 16 tiles, each
    ; tile 8 bytes into the BG Map16 table.
    LDA !Row
    ASL A
    ASL A
    ASL A
    ASL A
    STA !Offset
    LDA !Column
    AND #$000F
    ORA !Offset
    ASL A
    STA !Offset
    LDA !Column
    AND #$0010
    BEQ +
    LDA #!BgCacheRight
    CLC
    ADC !Offset
    STA !Offset
+   LDX !Offset
    LDA.l !BgCache,x
    ASL A
    ASL A
    ASL A
    CLC
    ADC !BgTable
    STA !Words
    SEP #$20
    LDA !BgTable+2
    STA !Words+2
    REP #$20
    RTS

; ---------------------------------------------------------------------------
; X = the layer. Uploads what its buffer holds for it, through the NMI's
; DMA channel, and clears its marks. A, X, Y 16-bit; direct page $00-$0D used.
upload_layer:
    LDA !Pending,x
    BNE +
    RTS
+   STX !Layer
    LDA #$3000
    CPX #$0000
    BEQ +
    LDA #$3800
+   STA $00                     ; the tilemap
    JSR buffer
    STA $02                     ; the buffer
    LDA !Pending,x
    AND #$0002
    BEQ .column
    ; Two tile rows, each RowCount words from RowColumn on, split where the
    ; tilemap's two halves meet and where it wraps.
    SEP #$20
    LDA #$80
    STA $2115
    REP #$20
    LDA !RowTile,x
    AND #$00FF
    STA $04
    JSR upload_tile_row
    LDX !Layer
    LDA !RowCount,x
    AND #$00FF
    ASL A
    CLC
    ADC $02
    STA $02
    LDA $04
    INC A
    AND #$001F
    STA $04
    JSR upload_tile_row
    LDX !Layer
    JSR buffer
    STA $02
.column:
    LDA !Pending,x
    AND #$0001
    BEQ .done
    ; Two tile columns, from ColumnRow down and round.
    SEP #$20
    LDA #$81
    STA $2115
    REP #$20
    LDA $02
    CLC
    ADC #!ColumnArea
    STA $02
    LDA !ColumnTile,x
    AND #$00FF
    STA $04
    JSR upload_tile_column
    LDA $02
    CLC
    ADC #!ColumnRight
    STA $02
    LDA $04
    INC A
    AND #$003F
    STA $04
    JSR upload_tile_column
.done:
    STZ !Pending,x
    RTS

; $00 the tilemap, $02 the words, $04 the tile row; X = the layer.
upload_tile_row:
    LDA !RowCount,x
    AND #$00FF
    STA $06                     ; words left
    LDA !RowColumn,x
    AND #$00FF
    STA $08                     ; the tile column
    LDA $02
    STA $0A                     ; the words' place
.segment:
    LDA $08
    AND #$0020
    ASL A
    ASL A
    ASL A
    ASL A
    ASL A
    ORA $00                     ; the half
    STA $0C
    LDA $04
    ASL A
    ASL A
    ASL A
    ASL A
    ASL A
    ORA $0C                     ; the row
    STA $0C
    LDA $08
    AND #$001F
    ORA $0C
    STA $2116
    LDA $08
    AND #$001F
    EOR #$FFFF
    SEC
    ADC #$0020                  ; words to the end of the half
    CMP $06
    BCC +
    LDA $06
+   STA $0C
    ASL A
    TAY
    JSR dma
    LDA $0C
    ASL A
    CLC
    ADC $0A
    STA $0A
    LDA $08
    CLC
    ADC $0C
    AND #$003F
    STA $08
    LDA $06
    SEC
    SBC $0C
    STA $06
    BNE .segment
    RTS

; $00 the tilemap, $02 the words, $04 the tile column; X = the layer.
upload_tile_column:
    LDA !ColumnRow,x
    AND #$00FF
    STA $08                     ; the first tile row
    LDA #$0020
    SEC
    SBC $08                     ; tile rows down to the tilemap's bottom
    STA $06
    LDA !ColumnRow,x
    XBA
    AND #$00FF                  ; tile rows in all
    CMP $06
    BCS +
    STA $06
+   LDA $02
    STA $0A
    JSR .segment
    LDA !ColumnRow,x            ; and from its top, the rest
    XBA
    AND #$00FF
    SEC
    SBC $06
    BEQ +
    BCC +
    PHA
    LDA $06
    ASL A
    CLC
    ADC $02
    STA $0A
    PLA
    STA $06
    STZ $08
    JSR .segment
+   RTS

.segment:
    LDA $04
    AND #$0020
    ASL A
    ASL A
    ASL A
    ASL A
    ASL A
    ORA $00
    STA $0C
    LDA $08
    ASL A
    ASL A
    ASL A
    ASL A
    ASL A
    ORA $0C
    STA $0C
    LDA $04
    AND #$001F
    ORA $0C
    STA $2116
    LDA $06
    ASL A
    TAY
    JMP dma

; Y = bytes. Sends them from $00:[$0A] through the NMI's DMA channel to
; VRAM, from where the address register points. X kept.
dma:
    SEP #$20
    LDA #$01                    ; two registers, $2118 and $2119
    STA !dma
    LDA #$18
    STA !dma+1
    LDA #$00
    STA !dma+4
    REP #$20
    LDA $0A
    STA !dma+2
    TYA
    STA !dma+5
    SEP #$20
    LDA #!dma_bit
    STA $420B
    REP #$20
    RTS

; ---------------------------------------------------------------------------
; From GenerateTile at $00BF37: A 8-bit, X and Y 16-bit, the block's
; position in $0C (X) and $0E (Y). Leaves the block's VRAM address in the
; game's tilemaps in $06 (high byte) and $07 (low), and X and Y, which the
; game keeps as _8 and _A, such that CODE_00C0FB's checks pass only for a
; block in the window below: equal to the block's position, or $7FF0, past
; any.
;
; The game's tilemaps are 64x64, layer 1's at $2000 and layer 2's at $3000:
; a Map16 cell's row's bit 4 is the address's bit 11, its column's bit 10,
; its row's low bits 6-9 and its column's 1-4. A row past 31, which only a
; taller level has, wraps; stripe_remap takes the window's rows apart.
tile_address:
    PHB
    PHK
    PLB
    REP #$30
    LDA !LayerProcessing
    AND #$00FF
    ASL A
    TAX
    LDA $0E
    AND #$0100
    ASL A
    ASL A
    ASL A
    STA $00                     ; the row's bit 4
    LDA $0C
    AND #$0100
    ASL A
    ASL A
    TSB $00                     ; the column's
    LDA $0E
    AND #$00F0
    ASL A
    ASL A
    TSB $00
    LDA $0C
    AND #$00F0
    LSR A
    LSR A
    LSR A
    ORA $00
    ORA #$2000
    CPX #$0000
    BEQ +
    ORA #$1000
+   SEP #$20
    STA $07
    XBA
    STA $06
    REP #$20
    ; Queued for the game's window: rows Y/16-8 to Y/16+23, and on a
    ; layer that scrolls horizontally columns X/16-8 to X/16+23 (32 of
    ; each, the game's tilemaps' height and width), as Lunar Magic's patch
    ; queues them.
    JSR layer_kind
    STA $02
    TXA
    ASL A
    TAY
    LDA $001C|!dp,y
    JSR shift4
    STA $00
    LDA $0E
    JSR shift4
    SEC
    SBC $00
    CLC
    ADC #$0008
    CMP #$0020
    BCS .hidden
    LDA $02
    CMP #!Horizontal
    BNE .shown
    LDA $001A|!dp,y
    JSR shift4
    STA $00
    LDA $0C
    JSR shift4
    SEC
    SBC $00
    CLC
    ADC #$0008
    CMP #$0020
    BCS .hidden
.shown:
    LDA $0C
    AND #$FFF0
    TAX
    LDA $0E
    AND #$FFF0
    TAY
    PLB
    JML $00BFB2
.hidden:
    LDX #$7FF0
    LDY #$7FF0
    PLB
    JML $00BFB2

; ---------------------------------------------------------------------------
; At the end of the game loop, in a level (game modes $05, $07, $13, and
; $14, the title screen's too): each stripe image queued for the game's
; layer 1 or layer 2 tilemap ($2000-$2FFF, $3000-$3FFF) goes to its place
; in these, its address's low 11 bits at $3000 or $3800, when its Map16 row
; (mod 32) is one of cy to cy+14 that the frame's builds keep; else it is
; taken out of the buffer, which a row of these tilemaps would show in its
; place. Returns to the game loop after its JSR RunGameMode.
!Stripes = $7F837D              ; the game's buffer of stripe images
!StripesEnd = $7F837B           ; where its last one ends ($FF there)

stripe_remap:
    PHP
    SEP #$20
    LDA $0100|!addr
    CMP #$14
    BEQ .level
    CMP #$13
    BEQ .level
    CMP #$07
    BEQ .level
    CMP #$05
    BEQ .level
    PLP
    JML $008075
.level:
    PHB
    PHK
    PLB
    REP #$30
    LDA.w #!Stripes
    STA $0A
    LDA.w #!Stripes>>8
    STA $0B
    LDY #$0000
.next:
    LDA [$0A],y
    BIT #$0080
    BEQ +
    JMP .done                   ; the end
+   XBA
    STA $00                     ; the address
    INY
    INY
    LDA [$0A],y
    DEY
    DEY
    XBA
    BIT #$4000
    BEQ +
    LDA #$0002                  ; a run of one word
    BRA ++
+   AND #$3FFF
    INC A
++  CLC
    ADC #$0004
    STA $04                     ; the entry's length
    LDA $00
    CMP #$2000
    BCC .skip
    CMP #$4000
    BCS .skip
    ; The layer's camera row, cy.
    LDX #$0000
    AND #$1000
    BEQ +
    LDX #$0004
+   LDA $001C|!dp,x
    INC A
    JSR shift4
    STA $02
    ; The Map16 row, mod 32, less cy.
    LDA $00
    LSR A
    LSR A
    LSR A
    LSR A
    LSR A
    LSR A
    AND #$000F
    STA $06
    LDA $00
    AND #$0800
    BEQ +
    LDA #$0010
    TSB $06
+   LDA $06
    SEC
    SBC $02
    AND #$001F
    CMP #$000F
    BCS .remove
    LDA $00
    AND #$07FF
    ORA #$3000
    CPX #$0000
    BEQ +
    ORA #$0800
+   XBA
    STA [$0A],y
.skip:
    TYA
    CLC
    ADC $04
    TAY
    JMP .next
.remove:
    ; What follows the entry, the end's $FF too, moves back over it.
    STY $06
    LDA.l !StripesEnd
    SEC
    SBC $06
    SEC
    SBC $04                     ; bytes after the entry, less the $FF
    PHA
    LDA.l !StripesEnd
    SEC
    SBC $04
    STA.l !StripesEnd
    TYA
    CLC
    ADC.w #!Stripes
    PHA
    CLC
    ADC $04
    TAX                         ; from the entry's end
    PLY                         ; to its start
    PLA                         ; bytes - 1
    PHB
    MVN !Stripes>>16,!Stripes>>16
    PLB
    LDY $06
    JMP .next
.done:
    PLB
    PLP
    JML $008075

; $0FFFE6 = $01, as Lunar Magic's install leaves it. Community patches take
; anything but $00 and $FF there to mean its VRAM patch is in, and refuse to
; assemble without it (docs/lunar-magic-install.md, "Bytes Kobo writes
; because Lunar Magic or other tools check them"). Lunar Magic's save
; writes the same.
org $0FFFE6
    db $01
