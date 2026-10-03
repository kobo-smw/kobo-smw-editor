; Screen exits and secondary entrances in Lunar Magic's format.
;
; Kobo's own code, written from the formats the community documents
; (docs/lunar-magic.md), the vanilla code it hooks, and where a Lunar
; Magic-saved ROM's entrance code sends each kind of exit
; (examples/exit_probe.rs), never from Lunar Magic's code.
;
; An exit's second byte is 0000wush: in Lunar Magic's format (u), h is the
; destination's bit 8, s makes it a secondary exit, and w makes the
; secondary entrance a water level, or without s leads to the destination's
; midway entrance (entrance.asm); in the game's, bit 0 is kept and bit 1
; is the level's secondary flag. A secondary entrance's destination has its
; bit 8 in bit 3 of its $05FE00 byte (IPXXDAAA). An exit in the game's
; format keeps the game's rule, the destination's bit 8 from the player's
; submap.
;
; Lunar Magic's long screen exit, extended object 02, has the destination's
; low byte in its fourth byte and nnnnwush in its fifth, nnnn the
; destination's bits 9-12, for secondary entrances up to 1FFF; the object
; leaves the fifth byte whole in $19D8, and the exit takes the destination
; as nnnnh and the low byte, a normal exit's too (exit_probe on a Lunar
; Magic-saved ROM, docs/lunar-magic.md). Every secondary entrance is read
; through the pointers Lunar Magic keeps at fixed addresses for the four
; tables ($0DE191, $0DE198, $0DE19F, $05DC81): entrance.asm points them
; at the game's tables, entrances.asm at tables of $2000 it moves.
;
; Lunar Magic's first save points $05D7CE at its own code and writes its
; own over the other sites, reading the same data.

incsrc "memory.asm"

; The screen exit object keeps the flags nibble for the screen, and the
; level's secondary flag is the exit's s bit.
org $0DA532 : db $0F            ; AND #$01
org $0DA536 : AND #$02          ; LDA $0B, before LSR : STA $1B93

; The destination's bit 8: BEQ : LDA #$01 after the submap's LDA.
org $05D7CE
    autoclean JSL exit_high

; A secondary entrance's four bytes, which the game reads from its own
; tables after LDY $0E, from there to the JMP to CODE_05D8B7: A 8-bit, X
; and Y 16-bit, the data bank $05, B 0. Lunar Magic's save writes its own
; from $05D7E2 too, keeping the LDY.
org $05D7E2
    JML secondary_exit
org $05D83B
secondary_exit_done:            ; JMP CODE_05D8B7

; The long screen exit in the extended objects' dispatch (CODE_0DA106),
; which runs it with RTS to bank $0D: a stub past exlevel.asm's.
org $0DA10F+(2*3) : dl long_exit_stub
org $0DFFCA
long_exit_stub:
    JSL long_exit
    RTS
assert pc() <= $0E0000

freecode

; A (8-bit) = the player's submap, X = the exit's screen. Returns A = the
; destination's bit 8; leaves the exit's flags in $02 for entrance_type and
; in $0BDC for entrance.asm (w without s leads to the midway entrance). An
; exit in the game's format, and Yoshi's wings and the bonus game
; (CODE_05DBAC) whatever the screen's flags, take bit 8 from the translevel,
; levels from 101 on being translevels $25 and up, where the game takes it
; from the submap, as in a Lunar Magic-saved ROM (exit_probe,
; docs/lunar-magic-install.md, "Entrances, exits, and midway points"); for vanilla's overworld the two
; agree.
exit_high:
    LDA $1B95|!addr             ; Yoshi's wings
    ORA $1425|!addr             ; the bonus game
    BNE .game
    LDA $19D8|!addr,x
    BIT #$04
    BNE .lunar_magic
.game:
    STZ $02
    STZ $0BDC|!addr
    LDA $13BF|!addr
    CMP #$25
    LDA #$00
    ROL A
    RTL
.lunar_magic:
    STA $02
    STA $0BDC|!addr
    AND #$02
    LSR A
    STA $1B93|!addr             ; secondary, by this exit
    LDA $02                     ; nnnnwush: the destination's bits 8-12
    LSR A
    LSR A
    LSR A
    AND #$1E
    PHA
    LDA $02
    AND #$01
    ORA $01,s
    STA $01,s
    PLA
    RTL

; Extended object 02, a long screen exit: $0A its first byte, the screen,
; and [$65] at its fourth. Leaves the fourth byte in $19B8 and the fifth in
; $19D8, for the screen, and $65 past the object, as the game's screen
; exit does; the level's secondary flag is left as it is. A, X, and Y
; 8-bit.
long_exit:
    LDA $0A
    AND #$1F
    TAX
    LDY #$00
    LDA [$65],y
    STA $19B8|!addr,x
    INY
    LDA [$65],y
    STA $19D8|!addr,x
    REP #$21
    LDA $65
    ADC #$0002
    STA $65
    SEP #$20
    RTL

; A secondary exit to the entrance in Y, from $0E-$0F (bits 8-12 in $0F):
; its four bytes, read as the game reads them from its own tables (CODE_05D796),
; from wherever the pointers say they are, with $04-$06 the pointer.
secondary_exit:
    LDX #$0000
    JSR entrance_byte
    STA $0E
    STA $17BB|!addr
    LDX #$0007
    JSR entrance_byte
    STA $00                     ; the player's Y and the layers' positions
    AND #$0F
    TAX
    LDA.l $05D730,x
    STA $96
    LDA.l $05D740,x
    STA $97
    LDA $00
    AND #$30
    LSR A
    LSR A
    LSR A
    LSR A
    TAX
    LDA.l $05D708,x
    STA $1C
    LDA $00
    LSR A
    LSR A
    LSR A
    LSR A
    LSR A
    LSR A
    TAX
    LDA.l $05D70C,x
    STA $20
    LDX #$000E
    JSR entrance_byte
    STA $01                     ; the player's X
    LSR A
    LSR A
    LSR A
    LSR A
    LSR A
    TAX
    LDA.l $05D750,x
    STA $94
    LDA.l $05D758,x
    STA $95
    LDA.l $05DC81
    STA $04
    LDA.l $05DC82
    STA $05
    LDA.l $05DC83
    STA $06
    LDA [$04],y                 ; IPXXDAAA
    JSL entrance_type
    JML secondary_exit_done

; Entrance Y's byte from the table whose pointer is X bytes past
; $0DE191, with A 8-bit, so that B stays 0.
entrance_byte:
    LDA.l $0DE191,x
    STA $04
    LDA.l $0DE192,x
    STA $05
    LDA.l $0DE193,x
    STA $06
    LDA [$04],y
    RTS

; A (8-bit) = the entrance's $05FE00 byte, Y (16-bit) its number. Sets $0F
; to its destination's bit 8 and $192A to its action, with $40 for an exit's
; water bit, and leaves the number in $0BDA for entrance.asm.
entrance_type:
    STY $0BDA|!addr
    PHA
    LSR A
    LSR A
    LSR A
    AND #$01
    STA $0F
    LDA $02
    AND #$08                    ; w, of an exit in Lunar Magic's format
    ASL A
    ASL A
    ASL A
    STA $02
    PLA
    AND #$07
    ORA $02
    STA $192A|!addr
    RTL
