; Lunar Magic's layer 3 settings, which it keeps in the high nibbles of a
; level's graphics list: how layer 3 scrolls, where it starts, colour math
; and the subscreen for it, the scroll sync fix, and what sprites beyond the
; level's edges touch.
;
; Kobo's own code, written from the format the community documents
; (smwspeedruns' level data format, "ExGFX Files"), Lunar Magic's help
; ("Change Layer 3 Settings"), the vanilla code it hooks, and what a Lunar
; Magic-saved ROM's levels leave in RAM when loaded and on every frame of play
; (docs/lunar-magic-install.md, "Layer 3 settings"), never from Lunar Magic's
; code. Needs graphics.asm, which leaves where the level's list is at
; $7FC006.
;
; The nibbles, by slot: BG3 AAAA (what tides act like), SP1 SCXX, SP2 HVYY,
; SP3 yyyy, LG1 vvvv, LG2 hhhh, LG3 YYYY, LG4 yOIB. With B ("advanced
; bypass") the level's layer 3 scrolls by Hhhhh horizontally and Vvvvv
; vertically from X (XX: 0, 4, 8, or 16 tiles) and Y (an 11-bit signed
; number of 16-pixel tiles, YYyyyyYYYYy from the top), C puts layer 3 in
; colour math and S on the subscreen, and I computes layer 3's position a
; frame behind. O makes a sprite beyond the level's edges touch air rather
; than water, with B or without. Without B the others change nothing.
;
; The check Lunar Magic makes: a JSL at $00A01F; with anything else there,
; a save installs its own layer 3 code.

incsrc "memory.asm"

!List = $0FF7FF

; RAM the load leaves, as a Lunar Magic-saved ROM's leaves it: the list's
; nibbles (LG3 and LG4, LG1 and LG2, SP1, BG3, SP2 and SP3), and with B the
; scroll settings times two, the offsets, the speeds of the scrolling modes
; and their fractions ($1458-$145D, as the game keeps its own layer 3
; autoscroll), and with I the position a frame ahead.
!Flags = !addr|$145E            ; LG3 << 4 | LG4: bit 0 B, bit 1 I, bit 2 O
!HMode = !addr|$145F
!VMode = !addr|$1460
!XSpeed = !addr|$1458
!YSpeed = !addr|$145A
!XFraction = !addr|$145C
!YFraction = !addr|$145D
!XOffset = !addr|$146A
!YOffset = !addr|$146C
!XAhead = !addr|$1B78
!YAhead = !addr|$1B7A
!SP1 = $7FC01A
!BG3 = $7FC01B
!SP2SP3 = $7FC01C
; A byte a Lunar Magic-saved ROM leaves at 0, which the setup borrows and
; gives back at 0.
!Skip = $7FC01D

; ---------------------------------------------------------------------------
; The end of the game's layer 3 setup (CODE_009FB8), after it has set layer
; 3's position, its scroll type, and colour math for the level's layer 3
; setting, and before it loads that setting's tilemap: LDA $1BE3 : BEQ + :
; DEC A. Returns the same A and Z.
org $00A01F
    autoclean JSL setup
    BEQ setup_end
org $00A044
setup_end:                      ; the routine's RTS

; The game's layer 3 scroll each frame (CODE_05C40C), run while $13D5 is 0:
; LDA $1403 : BEQ +.
org $05C40C
    autoclean JSL scroll
    NOP

; A sprite beyond the level's edges (CODE_0194B4): LDA #$00 : STA $1693,
; then STA $1694 with the same A.
org $0194B6
    autoclean JSL edge_tile
    NOP

freecode

; Per scroll setting (0-1F): 0 none, 1 a fraction of layer 1's position
; (the shift in the high byte, 7 for 1.2), 2 autoscroll at the speed in
; speeds. Settings no dialog offers act as 1.
modes:
    db 0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    db 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1
shifts:
    db 0, 0, 1, 2, 5, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    db 0, 0, 0, 0, 0, 0, 0, 0, 3, 4, 6, 0, 0, 0, 0, 0
speeds:
    dw $0000, $0000, $0000, $0000, $0000, $0000, $0040, $0080
    dw $0100, $0200, $FFC0, $FF80, $FF00, $FE00, $FD00, $FC00
    dw $0300, $0400, $0000, $0000, $0000, $0000, $0000, $0000
    dw $0000, $0000, $0000, $0000, $0000, $0000, $0000, $0000
x_offsets:
    dw $0000, $0040, $0080, $0100

; A 8-bit, direct page $00-$02 in use by the caller.
setup:
    PHP
    REP #$30
    PHB
    PHK
    PLB
    PHX
    PHY
    LDA $00
    PHA
    LDA $04
    PHA
    LDA $06
    PHA
    LDA $08
    PHA
    SEP #$20
    LDA #$00
    STA.l !Skip
    REP #$20
    LDA.l !List+1
    CMP #$FFFF
    BNE +
    JMP .done
+   LDA.l $7FC006
    STA $04
    LDA.l $7FC007
    STA $05
    SEP #$20
    LDY.w #0*2+1                ; AN2's high byte: T
    LDA [$04],y
    AND #$20
    STA.l !Skip
    ; The nibbles, for this code and whatever else reads them.
    LDY.w #13*2+1               ; LG3
    LDA [$04],y
    AND #$F0
    STA $08
    LDY.w #12*2+1               ; LG4
    LDA [$04],y
    LSR A
    LSR A
    LSR A
    LSR A
    ORA $08
    STA !Flags
    LDY.w #15*2+1               ; LG1
    LDA [$04],y
    AND #$F0
    STA $08
    LDY.w #14*2+1               ; LG2
    LDA [$04],y
    LSR A
    LSR A
    LSR A
    LSR A
    ORA $08
    STA !HMode
    LDY.w #11*2+1               ; SP1, beside the bit the graphics loader
    LDA [$04],y                 ; leaves in bit 7 (a layer 3 tilemap)
    LSR A
    LSR A
    LSR A
    LSR A
    STA $08
    LDA.l !SP1
    AND #$F0
    ORA $08
    STA.l !SP1
    LDY.w #2*2+1                ; BG3
    LDA [$04],y
    LSR A
    LSR A
    LSR A
    LSR A
    STA.l !BG3
    JSR tide_tiles
    LDY.w #10*2+1               ; SP2
    LDA [$04],y
    AND #$F0
    STA $08
    LDY.w #9*2+1                ; SP3
    LDA [$04],y
    LSR A
    LSR A
    LSR A
    LSR A
    ORA $08
    STA.l !SP2SP3
    LDA !Flags
    LSR A
    BCS +
    JMP .done
+
    ; B: layer 3 scrolls by the list's settings.
    STZ $13D5|!addr
    ; Hhhhh and Vvvvv, times two.
    LDA !HMode
    AND #$0F
    STA $08
    LDA.l !SP2SP3
    AND #$80
    LSR A
    LSR A
    LSR A
    ORA $08
    ASL A
    STA !HMode
    LDA.l !SP2SP3
    AND #$40
    LSR A
    LSR A
    STA $08
    LDY.w #15*2+1               ; LG1
    LDA [$04],y
    LSR A
    LSR A
    LSR A
    LSR A
    ORA $08
    ASL A
    STA !VMode
    ; Colour math and the subscreen.
    LDA $40
    AND #$FB
    STA $40
    LDA.l !SP1
    AND #$04
    TSB $40
    LDA.l !SP1
    AND #$08
    BEQ +
    LDA $0D9D|!addr
    AND #$04
    TSB $0D9E|!addr
    TRB $0D9D|!addr
    LDA $0D9D|!addr             ; the game has written the registers already
    STA $212C
    LDA $0D9E|!addr
    STA $212D
+
    REP #$20
    ; The offsets: X from XX, Y from YYyyyyYYYYy, in tiles of 16.
    LDA.l !SP1
    AND #$0003
    ASL A
    TAX
    LDA.l x_offsets,x
    STA !XOffset
    LDA.l !SP2SP3               ; YYyyyy, the high six bits
    AND #$003F
    ASL A
    ASL A
    ASL A
    ASL A
    ASL A
    STA $08
    LDA !Flags                  ; YYYY, then y
    AND #$00F0
    LSR A
    LSR A
    LSR A
    ORA $08
    STA $08
    LDA !Flags
    AND #$0008
    BEQ +
    INC $08
+   LDA $08
    CMP #$0400
    BCC +
    ORA #$F800                  ; negative
+   ASL A
    ASL A
    ASL A
    ASL A
    STA !YOffset
    ; Speeds and fractions.
    LDA !HMode
    AND #$00FF
    TAX
    LDA.l speeds,x
    STA !XSpeed
    LDA !VMode
    AND #$00FF
    TAX
    LDA.l speeds,x
    STA !YSpeed
    SEP #$20
    STZ !XFraction
    LDA !XSpeed+1
    BPL +
    LDA !XSpeed
    STA !XFraction
+   STZ !YFraction
    LDA !YSpeed+1
    BPL +
    LDA !YSpeed
    STA !YFraction
+   REP #$20
    ; Where layer 3 starts: at its offset, or its offset from layer 1's
    ; position by a fraction.
    LDX #$0000
    JSR start
    LDX #$0002
    JSR start
    LDA !Flags
    AND #$0002
    BEQ .done
    LDA $22
    STA !XAhead
    LDA $24
    STA !YAhead
.done:
    REP #$30
    PLA
    STA $08
    PLA
    STA $06
    PLA
    STA $04
    PLA
    STA $00
    PLY
    PLX
    PLB
    PLP
    ; What the hook replaces, but a list with T (layer 3's tilemap from
    ; LT3, loaded already) skips the level's layer 3 stripe image, which
    ; would draw over it.
    LDA.l !Skip
    BNE .skip
    LDA $1BE3|!addr
    BEQ +
    DEC A
    REP #$02                    ; Z clear
+   RTL
.skip:
    LDA #$00
    STA.l !Skip                 ; Z set
    RTL

; X = 0 for the horizontal axis, 2 for the vertical. A, X, Y 16-bit. In a
; tide level a vertical autoscroll starts at its offset from layer 1's
; position (Lunar Magic's help: the offsets are positions "except in tide
; levels with vertical auto-scroll"; observed so).
start:
    JSR mode
    CMP #$0001
    BNE .fixed
.fraction:
    JSR fraction
    BRA .store
.fixed:
    CMP #$0002
    BNE .offset
    CPX #$0002
    BNE .offset
    LDA $1403|!addr
    AND #$00FF
    BEQ .offset
    LDA !XOffset,x
    CLC
    ADC $1A,x
    BRA .store
.offset:
    LDA !XOffset,x
.store:
    STA $22,x
    RTS

; The axis X's setting: A its kind (modes), Y the setting. A, X, Y 16-bit.
mode:
    TXA
    LSR A
    TAY
    LDA !HMode,y                ; $145F horizontal, $1460 vertical
    AND #$00FF
    LSR A
    TAY
    LDA.w modes,y
    AND #$00FF
    RTS

; Layer 1's position on the axis X by the fraction of setting Y, plus the
; axis's offset. A, X, Y 16-bit.
fraction:
    LDA.w shifts,y
    AND #$00FF
    CMP #$0007
    BEQ .fast
    STA $00
    LDA $1A,x
-   DEC $00
    BMI +
    CMP #$8000
    ROR A
    BRA -
+   CLC
    ADC !XOffset,x
    RTS
.fast:
    ; 1.2 times: a fifth of the position (unsigned) added to the position
    ; and offset, with the carry that sum leaves (so a negative offset the
    ; position passes counts one more).
    LDA $1A,x
    JSR fifth
    STA $00
    LDA $1A,x
    CLC
    ADC !XOffset,x
    ADC $00
    RTS

; A (16-bit, unsigned) divided by 5. X kept.
fifth:
    PHX
    STA $4204
    SEP #$20
    LDA #$05
    STA $4206
    NOP #4
    REP #$20
    NOP #4
    LDA $4214
    PLX
    RTS

; ---------------------------------------------------------------------------
; Each frame: with B, layer 3's position from the list's settings, then the
; routine's end; otherwise what the hook replaces, returning to the game's
; code for a tide or for the rest.
scroll:
    PHP
    SEP #$20
    LDA !Flags
    LSR A
    BCS .bypass
    LDA $1403|!addr
    BNE .tide
    ; To the game's code after the BEQ ($05C414).
    REP #$20
    LDA $02,s
    CLC
    ADC #$0004
    STA $02,s
.tide:
    PLP
    RTL
.bypass:
    REP #$30
    PHB
    PHK
    PLB
    PHX
    PHY
    LDA $00
    PHA
    LDX #$0000
    JSR axis
    LDX #$0002
    JSR axis
    JSR tide_offsets
    PLA
    STA $00
    PLY
    PLX
    PLB
    ; To the routine's end ($05C491: SEP #$20 : RTS).
    LDA $02,s
    CLC
    ADC.w #$05C490-$05C40F
    STA $02,s
    PLP
    RTL

; In a tide level, the tide's interaction offsets from layer 1 as Lunar
; Magic's code leaves them with B, from layer 3's position as it now is
; (the game's own, CODE_05BC4A, would take it frames late): layer 3's
; position less layer 1's next, but horizontally $80 for an autoscroll.
; A, X, Y 16-bit.
tide_offsets:
    LDA.l $001403|!addr
    AND #$00FF
    BEQ .done
    LDX #$0000
    JSR mode
    LDX #$0000
    CMP #$0002
    LDA $22
    BCC +
    LDA #$0080
+   SEC
    SBC.l $001462|!addr
    STA $26
    LDA $24
    SEC
    SBC.l $001464|!addr
    STA $28
.done:
    RTS

; What a tide acts like (AAAA): the game fills rows 16-26 of each of layer
; 2's 16 screens with tile $000, water, for a tide (CODE_00A045, just run,
; which exlevel.asm moves to a size of the level's own as Lunar Magic's
; load does); with AAAA those rows take other tiles, in the low and high
; bytes, as a Lunar Magic-saved ROM's load leaves them: the same rows,
; rows 16 down of the screens from layer 2's first (exlevel.asm's
; pointers), the first always and the rest while a screen ends within
; $3700 bytes of the planes. A 8-bit, X and Y 16-bit; uses $08 and keeps
; $06 (the list's bank, in [$04]).
tide_tiles:
    LDA $1403|!addr
    BNE +
    RTS
+   LDA.l !BG3
    BNE +
    RTS
+   REP #$20
    PEI ($06)
    PEI ($0A)
    PEI ($0C)
    PEI ($0E)
    LDA.l !BG3
    AND #$000F
    DEC A
    ASL A
    ASL A
    TAY
    LDA.w tide_kinds,y
    STA $06
    LDA.w tide_kinds+2,y
    STA $08
    ; The screens: the game's, or with the taller levels' code those its
    ; pointers start layer 2's at, a vertical level's being the game's.
    LDA #$01B0
    STA $0A                     ; bytes a screen
    LDA.w #(!map16_low&$FFFF)+$1B00   ; screen 16
    STA $0C                     ; the first screen's address
    LDA.l $05DA8A
    AND #$00FF
    CMP #$0022
    BNE .first
    LDA $5B
    LSR A
    BCS .first
    LDA $13D7|!addr             ; the level's height
    STA $0A
    SEP #$20                    ; entry 16 of exlevel.asm's pointer bytes
    LDA !addr|$0CE6             ; for the block lookups ($0CD6, $0CB6)
    XBA
    LDA !addr|$0CC6
    REP #$20
    STA $0C
.first:
    LDA $0A
    CMP #$0101
    BCC .end                    ; no row 16
    LDA $0C
    BEQ .end                    ; past the planes
    CLC
    ADC $0A
    BCC .screen
    BNE .end                    ; the screen runs past them
.screen:
    LDA $0A
    SEC
    SBC #$0100
    STA $0E                     ; bytes of rows 16 down
    LDA $0C
    CLC
    ADC #$0100
    TAX
    LDY #$0000
.tile:
    LDA $06                     ; the top row
    CPY #$0010
    BCC +
    LDA $08                     ; the rows under it
+   SEP #$20
    STA.l !map16_low&$FF0000,x
    XBA
    STA.l !map16_high&$FF0000,x
    REP #$20
    INX
    INY
    CPY $0E
    BCC .tile
    LDA $0C
    CLC
    ADC $0A
    BCS .end
    STA $0C                     ; the next screen
    ADC $0A
    BCS .end
    CMP #$FF01                  ; ends within $FF00
    BCC .screen
.end:
    PLA
    STA $0E
    PLA
    STA $0C
    PLA
    STA $0A
    PLA
    STA $06
    SEP #$20
    RTS

; Per AAAA from 1: the top row's tile and the tile of the rows under it.
; 1 lava, 2 lava that kills only the player, 3 cave lava, 4 solid, 5-F
; tiles $200-$20A.
tide_kinds:
    dw $0004, $0005, $0005, $0005, $0159, $0005, $0130, $0130
    dw $0200, $0200, $0201, $0201, $0202, $0202, $0203, $0203
    dw $0204, $0204, $0205, $0205, $0206, $0206, $0207, $0207
    dw $0208, $0208, $0209, $0209, $020A, $020A

; One axis (X = 0 horizontal, 2 vertical). A, X, Y 16-bit.
axis:
    LDA !Flags
    AND #$0002
    BEQ .move
    ; I: the setting moves the position a frame ahead, and layer 3 takes
    ; the one that was ahead before.
    LDA $22,x
    PHA
    LDA !XAhead,x
    STA $22,x
    JSR .move
    LDA $22,x
    LDY !XAhead,x
    STA !XAhead,x
    TYA
    STA $22,x
    PLA
    RTS
.move:
    JSR mode
    CMP #$0001
    BEQ .fraction
    BCC .none
    ; Autoscroll: the speed added to the fraction as the game moves its own
    ; layer 3, then layer 1's movement this frame (Layer1DXPos,
    ; Layer1DYPos); nothing while sprites are frozen.
    LDA $9D
    AND #$00FF
    BNE .none
    TXA
    LSR A
    TAY                         ; 0 or 1, the fraction's byte
    LDA !XFraction,y
    AND #$00FF
    CLC
    ADC !XSpeed,x
    SEP #$20
    STA !XFraction,y
    REP #$20
    AND #$FF00
    BPL +
    ORA #$00FF
+   XBA
    CLC
    ADC $22,x
    STA $22,x
    LDA $17BD|!addr             ; horizontal
    CPX #$0000
    BEQ +
    LDA $17BC|!addr             ; vertical
+   AND #$00FF
    CMP #$0080
    BCC +
    ORA #$FF00
+   CLC
    ADC $22,x
    STA $22,x
.none:
    RTS
.fraction:
    JSR fraction
    STA $22,x
    RTS

; ---------------------------------------------------------------------------
; A sprite beyond the level's edges: the tile it touches is air ($25) with
; O, water ($00, the game's) without. Returns A = 0 for the STA $1694 after.
edge_tile:
    LDA #$00
    PHA
    LDA !Flags
    AND #$04
    BEQ +
    PLA
    LDA #$25
    PHA
+   PLA
    STA $1693|!addr
    LDA #$00
    RTL
