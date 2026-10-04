; Lunar Magic's entrance settings: the per-level tables ($05DE00, $06FA00,
; $06FC00, $06FE00), the separate midway entrance, and a secondary
; entrance's two further tables, in the formats the community documents
; (docs/lunar-magic-install.md, "Entrances, exits, and midway points").
;
; Kobo's own code, written from those formats, the vanilla code it hooks,
; and what a Lunar Magic-saved ROM's entrance code leaves in RAM for every
; value of every settings byte (examples/entry_probe.rs), never from Lunar
; Magic's code. The JSL at $05DA17 is Lunar Magic's check for the
; per-level tables, and a save keeps them with it there; every save writes
; its own code over the entrance code's sites, reading the same tables.
;
; Needs exits.asm, which leaves a secondary entrance's number in $0BDA and
; an exit's flags in $0BDC (work RAM the game does not use; the entrance
; code clears them once read. The sprite graphics buffer from $0BF6, where
; they were, holds the taller levels' tables, exlevel.asm, by then).

incsrc "memory.asm"

; The entrance code's end, where every kind of entrance has been set up:
; SEP #$30 : LDA $13BF.
org $05DA17
    autoclean JSL entry_settings
    NOP

; The midway entrance's screen: LSR #4 of the $05F400 byte.
org $05D9E3
    JSL midway_screen

; The entrance action's setup, after the level's RAM is cleared: LDA $1C :
; CMP #$C0.
org $00A6CC
    JSL entry_flags

; Game mode $11's screen count (LDA #$20 : STA $5E), after the player's
; entrance is set up and before layer 2's: the entrance's face left.
org $009708
    JSL entry_facing

; The player's interaction with layer 2, after the game's collision with
; it (CODE_00E938, from REP #$20 to SEP #$20 after it, A 8-bit): the
; offsets layer 2 interaction goes by trail a frame, as they do in a Lunar
; Magic-saved ROM (RAM, frame by frame).
org $00E978
after_layer2_collision:         ; ASL $13EF
org $00E966
    JML layer2_collision

; The slanted pipe's X speed (LDA #$40 in PipeCannonAni), which
; entry_facing leaves in $F9: to the left with face left.
org $00D2B2
    LDA $F9

; The midway tape sets the midway point on any screen: BEQ over it when
; $13CD is 0.
org $00F2DB
    NOP #2

; The camera's layer 2 (CODE_00F79D to CODE_00F7C2, A 16-bit): layer 2's
; position from layer 1's by the scroll settings, Lunar Magic's added ones
; too (below). Lunar Magic's save puts its own camera here, which reads
; the same settings.
org $00F79D
    JSL layer2_camera
    JMP $F7C2                   ; the routine's end, from SEP #$20

; A vertical level's horizontal camera (CODE_00F75C, A 16-bit, Y 8-bit):
; SEC : SBC $142C,Y, and no move when the player is at the edge it goes
; by, as the game's horizontal levels have it (their BEQ) and a Lunar
; Magic-saved ROM moves in vertical ones (RAM; lunar-magic-install.md).
org $00F77F
vertical_camera_moves:          ; STA $02
org $00F77B
    JML vertical_camera

; Layer 1's vertical camera with vertical scrolling off (CODE_00F7F4, the
; player above the screen's middle, not flying or climbing: LDY #$04 : BRA
; CODE_00F881, A 16-bit): the game's moves layer 1 up 6 pixels a frame at
; most and keeps it at $C0 or lower, its lowest position at the game's
; height. A Lunar Magic-saved ROM keeps it at the lowest position the
; routine was given ($04: a taller level's bottom, a vertical level's last
; screen) wherever it was (RAM, lunar-magic-install.md).
org $00F871
    JML vertical_scroll_off

; The game's layer 2 scroll settings by $05F000's high nibble (DATA_05D710
; vertical, DATA_05D720 horizontal): Lunar Magic's 8 to 11 are vertical
; 1/4, 1/8, 1/16, and 1/64, with horizontal 1/2.
org $05D718
    db $04, $05, $06, $07
org $05D728
    db $02, $02, $02, $02

; The per-level tables as a fresh install has them; a build writes them.
org $05DE00
    fillbyte $00 : fill $200        ; IWPXXtTT
org $06FA00
    fillbyte $20 : fill $200        ; SHCvvvvv
org $06FC00
    fillbyte $00 : fill $200        ; OFYYYYYY
org $06FE00
    fillbyte $1A : fill $200        ; RL-ooooo

freedata

; Separate midway settings, one byte per level in each of four tables:
; IWHMXAAA, yyyyxxxx, RLE-ffbb, -FYYYYYY. A build writes them.
midway_tables:
    fillbyte $00 : fill $800

; A secondary entrance's two further bytes, EFYYYYYY and RLW-----, one per
; entrance in each table, behind the pointers where Lunar Magic keeps them.
; A build writes them.
freedata
entrance_table_5:
    fillbyte $00 : fill $200
freedata
entrance_table_6:
    fillbyte $00 : fill $200

; Where Lunar Magic keeps the entrance tables' pointers, at fixed addresses
; the community's level format page gives (docs/lunar-magic-install.md,
; "Bytes Kobo writes because Lunar Magic checks them"). Its save and MWL
; export read a secondary entrance's two further tables only with these and
; the marker below; without the four vanilla ones, it reads the main tables
; from elsewhere. Kobo's code reads the pointers.
org $05DC81
    dl $05FE00
org $05DC86
    autoclean dl entrance_table_5
org $05DC8B
    autoclean dl entrance_table_6
org $0DE191
    dl $05F800
org $0DE198
    dl $05FA00
org $0DE19F
    dl $05FC00

; A marker Lunar Magic's save checks before it keeps the separate layer 2
; scroll settings ($06FA00's S and H, from 3.40): without it the save
; installs its own entrance and camera code over Kobo's and sets every
; level's $06FA00 to $20. With it, the save also leaves Kobo's code at the
; entrance, the camera, and a few more of its own sites
; (docs/lunar-magic-install.md, "Bytes Kobo writes because Lunar Magic
; checks them").
org $05DD7C
    db "LM"

; A marker 3.x installs carry, which the same check needs. With it, a save
; also writes a different value at $0FEFB4, whose meaning is unknown. Run a
; feature's Lunar Magic check with and without it (docs/testing.md).
org $03BD9C
    db "LM",$10,$01

freecode
prot midway_tables

; A (8-bit) = the level's $05F400 byte, X and Y 8-bit. Returns A = the
; midway entrance's screen. The table address sits $0A bytes in, where the
; community's format documentation finds it.
;
; A separate midway entrance is set up whole here: the player, the layers,
; $13CD, and $192A, as a Lunar Magic-saved ROM leaves them. A Lunar Magic
; save keeps this hook but puts its own code at $05DA17, after which they
; are the same; with Kobo's there, entry_settings leaves them.
midway_screen:
    LSR A
    LSR A
    LSR A
    LSR A
    REP #$10
    LDX $0E
    XBA
.table:
    LDA.l midway_tables,x
assert midway_screen_table == midway_screen+9
    STA $00
    LDA.l midway_tables+$400,x
    AND #$20
    BNE .redirect
    LDA $00
    AND #$20
    BNE .separate
    LDA $00
    AND #$10
    STA $00
    XBA
    ORA $00
    SEP #$10
    RTL
.separate:
    JSR midway
    LDA $95
    SEP #$10
    RTL
.redirect:
    ; The midway entrance of another level: start over with it.
    LDA.l midway_tables+$400,x
    AND #$01
    STA $0F
    LDA.l midway_tables+$200,x
    STA $0E
    SEP #$10
    PLA
    PLA
    PLA
    JML $05D8B7

; The entrance code left the entrance's RL-ooooo in $13CD: with L the
; player faces left, and the slanted pipe shoots him left ($00D2B2), as in a
; Lunar Magic-saved ROM (docs/lunar-magic-install.md, "The sites a save
; keeps with the marker"). $13CD is cleared after, as there.
; The game's CODE_00A796, the JSR after the hook, sets layer 2's vertical
; offset from layer 1 ($1417) by the game's rates: 1/8 of layer 1 for every
; vertical setting from 3 on. A Lunar Magic-saved ROM leaves it so only
; without R and for settings below $10 (RAM, docs/lunar-magic-install.md,
; "Layer 2 scroll settings"): with R it keeps the relative camera's
; (relative_camera), and a setting that moves layer 2 by itself ($10 on)
; starts from layer 2 less layer 1. The JSR is skipped then, and the one
; other thing it does done here.
entry_facing:
    ; The hook replaces LDA #$20 : STA $5E, the most screens the camera's
    ; first placement goes to; a Lunar Magic-saved ROM gives it the level's
    ; own count there (RAM, every level tried), the header's, which the load
    ; puts in $5D later (CODE_0584E3), so an entrance on the last screen
    ; starts with the camera at the level's end.
    LDA [$65]
    AND #$1F
    INC A
    STA $5E
    LDA #$40
    BIT $13CD|!addr
    BVC +
    STZ $76
    LDA #$C0
+   STA $F9
    LDA $13CD|!addr
    STZ $13CD|!addr
    ASL A
    BCS .relative
    LDA $1414|!addr
    CMP #$10
    BCC .done
    REP #$20
    LDA $20
    SEC
    SBC $1C
    STA $1417|!addr
    BRA .skip
.relative:
    REP #$20
.skip:
    LDA #$0080
    STA $142A|!addr             ; CODE_00A796's camera move trigger
    LDA $01,s
    CLC
    ADC #$0003                  ; past JSR CODE_00A796 at $00970C
    STA $01,s
    SEP #$20
.done:
    RTL

; Returns with layer 1's position compared with the level's bottom, where
; the code it replaces compares $1C with $C0: vertical scrolling at will
; starts off only there.
entry_flags:
    LDA $192A|!addr
    BPL +
    LDA #$80
    STA $86                     ; slippery
+   BIT $192A|!addr
    BVC +
    LDA #$01
    STA $85                     ; water
+   LDA $192A|!addr
    AND #$3F
    STA $192A|!addr
    REP #$20
    LDA $5B
    LSR A
    BCS +
    LDA $13D7|!addr             ; the level's height (exlevel.asm)
    SEC
    SBC #$00F0
    BRA ++
+   LDA $5E                     ; $5F: the vertical level's screen count
    AND #$FF00
    SEC
    SBC #$0100
++  CMP $1C
    SEP #$20
    RTL

entry_settings:
    REP #$30
    LDA $0E
    AND #$01FF
    TAX
    SEP #$20
    JSR layer2_settings
    LDA.l $05DE00,x
    STA $00
    AND #$04
    ASL A
    ASL A
    ASL A
    ASL A
    ASL A
    STA $02
    LDA $00
    AND #$03
    ORA $02
    STA $0BF4|!addr             ; tTT
    LDA $1B93|!addr
    BEQ +
    JSR secondary
    BRA .done
+   LDA $13CF|!addr
    BNE .overworld_midway       ; from the overworld, past the midway point
    LDA $141A|!addr
    BEQ .main
    LDA $0BDC|!addr
    AND #$0A
    CMP #$08
    BNE .main                   ; an exit whose w, without s, leads here
    LDA.l midway_tables+$400,x
    AND #$20
    BNE .main                   ; which a redirected midway entrance ignores
.midway:
    LDA.l midway_tables,x
    AND #$20
    BEQ .main
    JSR midway
    BRA .done
.overworld_midway:
    LDA.l midway_tables,x
    AND #$20
    BNE .done                   ; set up at $05D9E3 already
    ; The game's midway entrance puts its screen in X's high byte; a
    ; vertical level takes it as Y's, and the layers start on it.
    LDA $5B
    LSR A
    BCC .main
    LDA $95
    STA $97
    STA $1D
    LDA $1414|!addr
    CMP #$03
    BEQ +
    LDA $95
    STA $21
+   STZ $95
.main:
    JSR main
.done:
    ; The game's vertical positioning sets the layer 1 vertical scroll
    ; setting early, which the camera then uses before the level's own
    ; replaces it; with the layers placed relative to the player it stays
    ; clear.
    LDA $5B
    LSR A
    BCC +
    LDA $13CD|!addr
    BPL +
    STZ $1412|!addr
+   LDA #$40
    TSB $0BE7|!addr             ; no layer 2 interaction yet (layer2_collision)
    STZ $0BDA|!addr             ; what exits.asm left
    STZ $0BDB|!addr
    STZ $0BDC|!addr
    SEP #$30
    LDA $13BF|!addr
    RTL

main:
    LDA $00
    AND #$C0
    TSB $192A|!addr
    LDA.l $06FE00,x
    STA $13CD|!addr
    LDA $00
    AND #$20
    BEQ .camera
    LDA.l $05F200,x
    AND #$07
    STA $02
    LDA $00
    AND #$18
    ORA $02
    STA $02                     ; the X tile, 5 bits
    LDA.l $05F000,x
    AND #$0F
    STA $03                     ; the Y tile's low 4 bits
    LDA.l $06FC00,x
    AND #$3F
    STA $04                     ; and its high 6
    JSR place
.camera:
    LDA $13CD|!addr
    BPL .done
    LDA.l $06FC00,x
    AND #$40
    LSR A
    LSR A
    STA $02
    LDA.l $05F400,x
    AND #$0F
    ORA $02
    STA $02                     ; Fffbb
    JSR relative_camera
.done:
    RTS

; A secondary entrance, whose number exits.asm left in $0BDA, with Lunar
; Magic's two tables of its own behind $05DC86 (EFYYYYYY) and $05DC8B
; (RLW-----), and its bytes in the game's tables, read through the
; pointers exits.asm reads them through: IPXXDAAA ($05DC81), its X
; ($0DE19F) and Y ($0DE198) bytes. X and Y 16-bit.
secondary:
    REP #$20
    LDA.l $05DC86
    STA $0A
    LDA $0BDA|!addr
    TAY
    PHX
    SEP #$20
    LDA.l $05DC88
    STA $0C
    LDA [$0A],y
    STA $01                     ; EFYYYYYY
    REP #$20
    LDA.l $05DC8B
    STA $0A
    SEP #$20
    LDA.l $05DC8D
    STA $0C
    LDA [$0A],y
    STA $00                     ; RLW-----
    AND #$20
    ASL A
    STA $02
    JSR ipxxdaaa
    AND #$80
    ORA $02
    TSB $192A|!addr             ; water and slippery
    JSR ipxxdaaa
    AND #$40
    BEQ .camera
    JSR ipxxdaaa
    AND #$30
    LSR A
    STA $02
    LDX #$000E
    JSR entrance_byte           ; the X byte
    LSR A
    LSR A
    LSR A
    LSR A
    LSR A
    ORA $02
    STA $02                     ; the X tile, 5 bits
    LDX #$0007
    JSR entrance_byte           ; the Y byte
    AND #$0F
    STA $03
    LDA $01
    AND #$3F
    STA $04
    JSR place
.camera:
    LDX #$0007
    JSR entrance_byte
    LSR A
    LSR A
    LSR A
    LSR A
    STA $02
    LDA $01
    AND #$40
    LSR A
    LSR A
    ORA $02
    STA $02                     ; Fbbff
    PLX
    LDA.l $06FE00,x
    AND #$3F
    STA $03
    LDA $00
    AND #$C0
    ORA $03
    STA $13CD|!addr
    BPL +
    JMP relative_camera
+   RTS

; Entrance Y's $05FE00 byte, through $05DC81, with $0A-$0C the pointer.
ipxxdaaa:
    LDA.l $05DC81
    STA $0A
    LDA.l $05DC82
    STA $0B
    LDA.l $05DC83
    STA $0C
    LDA [$0A],y
    RTS

; Entrance Y's byte in the table whose pointer is X bytes past $0DE191.
entrance_byte:
    LDA.l $0DE191,x
    STA $0A
    LDA.l $0DE192,x
    STA $0B
    LDA.l $0DE193,x
    STA $0C
    LDA [$0A],y
    RTS

midway:
    LDA.l midway_tables,x
    STA $00
    AND #$C7
    STA $192A|!addr             ; the action, slippery and water
    LDA.l midway_tables+$400,x
    AND #$C0
    STA $02
    LDA.l $06FE00,x
    AND #$3F
    ORA $02
    STA $13CD|!addr
    LDA.l midway_tables+$200,x
    AND #$0F
    STA $02
    LDA $00
    AND #$08
    ASL A
    ORA $02
    STA $02                     ; the X tile, 5 bits
    LDA.l midway_tables+$200,x
    LSR A
    LSR A
    LSR A
    LSR A
    STA $03
    LDA.l midway_tables+$600,x
    AND #$3F
    STA $04
    LDA.l $05F400,x
    LSR A
    LSR A
    LSR A
    LSR A
    STA $95
    LDA $00
    AND #$10
    TSB $95                     ; the screen, 5 bits
    LDA $5B
    LSR A
    BCC +
    LDA $95                     ; a vertical level's screen is Y's
    STA $97
+   JSR place
    LDA $13CD|!addr
    BPL .fixed
    LDA.l midway_tables+$600,x
    AND #$40
    LSR A
    LSR A
    STA $02
    LDA.l midway_tables+$400,x
    AND #$0F
    ORA $02
    STA $02
    JMP relative_camera
.fixed:
    ; The game's initial positions, by ff and bb.
    REP #$20
    LDA.l midway_tables+$400,x
    PHX
    PHA
    AND #$000C
    LSR A
    LSR A
    TAX
    SEP #$20
    LDA.l $05D708,x
    STA $1C
    REP #$20
    PLA
    AND #$0003
    TAX
    SEP #$20
    LDA.l $05D70C,x
    STA $20
    PLX
    RTS

; A (16-bit): the level's bottom camera position, where the background's
; last row shows: its height less $F0 (exlevel.asm), or in a vertical
; level the top of its last screen.
level_bottom:
    LDA $5B
    LSR A
    BCS .vertical
    LDA $13D7|!addr
    SEC
    SBC #$00F0
    RTS
.vertical:
    LDA $5E                     ; $5F: the vertical level's screen count
    AND #$FF00
    SEC
    SBC #$0100
    RTS

; A (16-bit) scaled by vertical scroll setting Y (16-bit, not 0) as the
; camera scales layer 1's position (layer2_camera), the settings that do
; not scale it (9 on) as 1: a layer placed relative to the player takes
; them as following layer 1. Y is then $05 again.
scale:
    CPY #$0009
    BCC +
    LDY #$0001
+   JSR rate
    LDY $05
    RTS

; The same, keeping A's sign: arithmetic shifts, and for 1.2 a fifth of the
; magnitude.
scale_signed:
    CPY #$0009
    BCC +
    LDY #$0001
+   CMP #$8000
    BCC +
    EOR #$FFFF
    INC A
    JSR rate_signed
    EOR #$FFFF
    INC A
    BRA ++
+   JSR rate
++  LDY $05
    RTS

; The negative of A's negative scaled by rate Y: shifts round towards minus
; infinity, as arithmetic shifts do.
rate_signed:
    CPY #$0008
    BNE +
    JMP rate
+   DEC A
    JSR rate
    INC A
    RTS

; A (16-bit): $0F with the level size's B (exlevel.asm), else 0; Z set
; without it.
bottom_row:
    LDA $0BF5|!addr
    AND #$0040
    BEQ +
    LDA #$000F
+   RTS

; The player's position from an entrance's tile: $02 the X tile (5 bits),
; $03 and $04 the Y tile's low 4 and high 6 bits. A horizontal level keeps
; the screen in $95 and takes 4 bits of X; a vertical one keeps it in $97
; and takes the Y tile's low 4 bits only.
place:
    LDA $5B
    LSR A
    BCS .vertical
    LDA $02
    ASL A
    ASL A
    ASL A
    ASL A
    STA $94
    REP #$20
    LDA $04
    AND #$003F
    ASL A
    ASL A
    ASL A
    ASL A
    STA $06
    LDA $03
    AND #$000F
    ORA $06
    ASL A
    ASL A
    ASL A
    ASL A
    STA $96
    SEP #$20
    RTS
.vertical:
    REP #$20
    LDA $02
    AND #$001F
    ASL A
    ASL A
    ASL A
    ASL A
    STA $94
    SEP #$20
    LDA $03
    ASL A
    ASL A
    ASL A
    ASL A
    STA $96
    RTS

; The layers' positions from the player's, for an entrance that sets them
; relative to the player: $02 the offset in rows (5 bits, signed), X the
; level. The background follows the level's settings: its height, or an
; offset from the foreground.
relative_camera:
    REP #$20
    LDA $1414|!addr
    AND #$00FF
    STA $05                     ; the vertical scroll setting, for Y
    LDA $02
    AND #$001F
    EOR #$0010
    SEC
    SBC #$0010
    ASL A
    ASL A
    ASL A
    ASL A
    CLC
    ADC $96
    BPL +
    LDA #$0000
+   BIT $0BF4|!addr             ; the level size's B (exlevel.asm): a pixel
    BVC +                       ; higher
    DEC A
+   STA $1C
    LDA.l $06FC00,x
    BIT #$0080
    BEQ .height
    LDA.l $06FE00,x
    AND #$001F
    CMP #$0010
    BEQ .absolute
    EOR #$0010
    SEC
    SBC #$0010
    ASL A
    ASL A
    ASL A
    ASL A
    CLC
    ADC $1C
    STA $20
    SEC
    SBC $1C
    LDY $05
    BEQ .offset
    BRA .scrolled
.absolute:
    STZ $20
    LDA #$0000
    SEC
    SBC $1C
    LDY $05
    BEQ .offset
    BRA .scrolled
.height:
    ; The background's bottom row where the level's is: the level's bottom
    ; camera position is its height less $F0, or in a vertical level the
    ; top of its last screen; the background's is its height less $F0.
    ; The background moves from there as the camera would move it: by
    ; layer 1's position scaled by the vertical scroll rate (logical
    ; shifts, as the camera's), less the bottom's scaled the same way.
    ; With the level size's B, the camera stops $0F lower and the
    ; background follows: $0F lower, or $10 with no scrolling.
    JSR level_bottom
    STA $02
    JSR bottom_row
    CLC
    ADC $02
    STA $02                     ; the bottom
    LDA $1C
    LDY $05
    BEQ .fixed
    JSR scale
    STA $07
    ; The bottom, with B, scaled keeping its sign (a size whose bottom is
    ; above 0 has it at -1 then).
    JSR bottom_row
    BEQ +
    LDA $02
    JSR scale_signed
    BRA ++
+   LDA $02
    JSR scale
++  EOR #$FFFF
    SEC
    ADC $07
    STA $02
    JSR bottom_row
    CLC
    ADC $02
    BRA .store
.fixed:
    JSR bottom_row
    BEQ .store
    INC A                       ; $10
.store:
    STA $02
    LDA.l $06FE00,x
    AND #$001F
    ASL A
    ASL A
    ASL A
    ASL A
    SEC
    SBC #$00E0
    CLC
    ADC $02
    STA $20
    LDY $05
    BEQ .done
.scrolled:
    ; The layer 2 offset the camera keeps from here on.
    LDA $1C
    JSR scale
    EOR #$FFFF
    SEC
    ADC $20
.offset:
    STA $1417|!addr
.done:
    SEP #$20
    RTS

; ---------------------------------------------------------------------------
; Layer 2's scroll settings. The game's are 0 (none), 1 (as layer 1), 2
; (1/2), and 3 (1/32, vertical only), paired by $05F000's high nibble.
; Lunar Magic adds, observed from what its camera does with each
; (docs/lunar-magic-install.md, "Layer 2 scroll settings"): 4 to 7 (1/4,
; 1/8, 1/16, 1/64), 8 (1.2 times), and 3 (1/32) horizontally too; with S
; ($06FA00 bit 7) the horizontal setting is the nibble, plus $10 with H
; (bit 6), and the vertical one the byte's low five bits. On either axis
; $10-$15 and $16-$1B move layer 2 by themselves (1/4, 1/2, 1, 2, 3, and 4
; pixels a frame, then the same the other way), following layer 1 as it
; moves too; $1C-$1F only follow it; 9-$F do not move it. A moving
; setting keeps its speed in $144A (horizontal) or $144C (vertical), its
; fraction in $1443 or $1445, and how far it has gone in $1452 or $1417,
; where a Lunar Magic-saved ROM has them (RAM).

; The level's settings, at the entrance: X (16-bit) the level, A 8-bit,
; Y 16-bit.
layer2_settings:
    LDA.l $06FA00,x
    BPL .moving
    PHA
    AND #$1F
    STA $1414|!addr
    LDA.l $05F000,x
    LSR A
    LSR A
    LSR A
    LSR A
    STA $1413|!addr
    PLA
    AND #$40
    BEQ .moving
    LDA $1413|!addr
    ORA #$10
    STA $1413|!addr
.moving:
    PHY
    LDY #$0000
    JSR layer2_speed
    LDY #$0002
    JSR layer2_speed
    PLY
    RTS

; Axis Y's (0 horizontal, 2 vertical) speed and fraction, if its setting
; moves layer 2: the fraction starts at the speed's low byte when it moves
; up or left. A 8-bit, X and Y 16-bit; X kept.
layer2_speed:
    PHX
    REP #$20
    TYA
    LSR A
    TAX
    SEP #$20
    LDA $1413|!addr,x
    SEC
    SBC #$10
    CMP #$0C
    BCS .done
    ASL A
    REP #$20
    AND #$00FF
    TAX
    LDA.l layer2_speeds,x
    TYX
    STA $144A|!addr,x
    LDA $144A|!addr,x
    BMI +
    LDA #$0000
+   SEP #$20
    STA $1443|!addr,x
.done:
    PLX
    RTS

layer2_speeds:
    dw $0040, $0080, $0100, $0200, $0300, $0400
    dw $FFC0, $FF80, $FF00, $FE00, $FD00, $FC00

; After the player's collision with layer 2, whose offsets from layer 1
; ($26, $28) the game added to the player's position: the position back,
; as the game's code has it, and then the offsets and layer 2's movement
; this frame ($17BE, $17BF) swapped with the previous frame's, kept at
; $0BE8-$0BED, so that what comes after (and the next frame's collision)
; goes by the previous frame's. The first time in a level, with $0BE7's bit
; 6 (set at the entrance), this frame's offsets are kept and stay, and the
; movement is swapped as ever (with none before: found in tide levels whose
; first collision comes after an entrance pipe, 2026-10-04). Learned from
; what a Lunar Magic-saved ROM leaves in those words frame by frame
; (docs/lunar-magic-install.md, "The sites a save keeps with the marker").
layer2_collision:
    REP #$20
    LDA $94
    SEC
    SBC $26
    STA $94
    LDA $96
    SEC
    SBC $28
    STA $96
    LDA $0BE7|!addr
    AND #$0040
    BEQ .swap
    LDA #$0040
    TRB $0BE7|!addr
    LDA $26
    STA $0BE8|!addr
    LDA $28
    STA $0BEA|!addr
    BRA .movement
.swap:
    LDA $26
    PHA
    LDA $0BE8|!addr
    STA $26
    PLA
    STA $0BE8|!addr
    LDA $28
    PHA
    LDA $0BEA|!addr
    STA $28
    PLA
    STA $0BEA|!addr
.movement:
    LDA $17BE|!addr
    PHA
    LDA $0BEC|!addr
    STA $17BE|!addr
    PLA
    STA $0BEC|!addr
.done:
    SEP #$20
    JML after_layer2_collision

; A vertical level's horizontal camera, from the player's X on the screen
; in A: less the edge it goes by ($142C or $142E by Y), and to the layers'
; scroll settings ($00F79D) when that is 0.
vertical_camera:
    SEC
    SBC $142C|!addr,y
    BEQ +
    JML vertical_camera_moves
+   JML $00F79D

; Vertical scrolling off: layer 1 at the lowest position, then the
; routine's RTS ($00F8AA).
vertical_scroll_off:
    LDA $04
    STA $1C
    JML $00F8AA

; In place of the camera's own layer 2 (CODE_00F79D): A 16-bit, X and Y
; 8-bit.
layer2_camera:
    PHP
    REP #$30
    PHX
    PHY
    LDX #$0000
    JSR layer2_axis
    LDX #$0002
    JSR layer2_axis
    PLY
    PLX
    PLP
    RTL

; Layer 2's position on axis X (0 horizontal, 2 vertical) from layer 1's:
; scaled by the setting, plus $1417 vertically as the game has it; or
; layer 1's plus how far a moving setting has gone, which moves first but
; while sprites are frozen ($9D). A, X, and Y 16-bit.
layer2_axis:
    TXA
    LSR A
    TAY
    LDA $1413|!addr,y
    AND #$00FF
    TAY
    BEQ .done
    CPY #$0009
    BCC .scaled
    CPY #$0010
    BCC .done
    CPY #$001C
    BCS .follow
    LDA $9D
    AND #$00FF
    BNE .follow
    LDA $0100|!addr
    AND #$00FF
    CMP #$0013
    BCC .follow
    LDA $1443|!addr,x
    AND #$00FF
    CLC
    ADC $144A|!addr,x
    SEP #$20
    STA $1443|!addr,x
    REP #$20
    AND #$FF00
    BPL +
    ORA #$00FF
+   XBA
    CPX #$0000
    BNE +
    CLC
    ADC $1452|!addr
    STA $1452|!addr
    BRA .follow
+   CLC
    ADC $1417|!addr
    STA $1417|!addr
.follow:
    LDA $1452|!addr
    CPX #$0000
    BEQ +
    LDA $1417|!addr
+   CLC
    ADC $1A,x
    STA $1E,x
.done:
    RTS
.scaled:
    LDA $1A,x
    JSR rate
    CPX #$0000
    BEQ +
    CLC
    ADC $1417|!addr
+   STA $1E,x
    RTS

; A (16-bit, unsigned) scaled by setting Y (1-8): shifted right, or for 8
; 1.2 times, a fifth added. A, X, and Y 16-bit; X and Y kept.
rate:
    CPY #$0008
    BEQ .fast
    PHX
    PHA
    TYX
    LDA.l rate_shifts,x
    AND #$00FF
    TAX
    PLA
    CPX #$0000
    BEQ +
-   LSR A
    DEX
    BNE -
+   PLX
    RTS
.fast:
    PHX
    PHA
    JSR fifth
    CLC
    ADC $01,s
    PLX
    PLX
    RTS

rate_shifts:
    db 0, 0, 1, 5, 2, 3, 4, 6

; A (16-bit, unsigned) divided by 5, in software: the SA-1 has no divider
; of the S-CPU's. A, X, and Y 16-bit; X, Y, and $00-$03 kept.
fifth:
    PHX
    PHY
    LDY $00
    PHY
    LDY $02
    PHY
    STA $00                     ; the quotient, as the dividend leaves
    STZ $02                     ; the remainder
    LDX #$0010
-   ASL $00
    ROL $02
    LDA $02
    SEC
    SBC #$0005
    BCC +
    STA $02
    INC $00
+   DEX
    BNE -
    LDA $00
    PLY
    STY $02
    PLY
    STY $00
    PLY
    PLX
    RTS
