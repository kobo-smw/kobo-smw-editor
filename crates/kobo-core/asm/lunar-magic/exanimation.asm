; Lunar Magic's ExAnimation: animated tiles and colours of a level's own and
; of every level (global), in its data format, with their triggers.
;
; Kobo's own code, written from the vanilla code it hooks (SMWDisX), the data
; format the community documents (smwspeedruns' level data format), Lunar
; Magic's help ("Edit Level ExAnimated Frames"), and what a Lunar
; Magic-saved ROM's levels do with chosen data frame by frame, observed as
; memory effects only (examples/exanim_probe.rs; docs/lunar-magic-install.md,
; "ExAnimation"), never from Lunar Magic's code.
;
; Data. A list (a level's, or the global one) is: the number of slots used
; (the highest plus one), the alternative file (ExGFX 60-63, as 0-3), which
; custom triggers keep their state at the load (a bit each, 16 bits; the
; others are cleared), which it sets, which manual triggers it sets (16
; bits), a frame for each of those in order, then a 16-bit offset per slot to its entry, counted from
; the offsets' start (0: an unused slot). An entry is its type, its
; trigger, its frames less one, a 16-bit destination (tiles: a VRAM word
; address, bit 15 for sources in the alternative file; colours: the first
; colour, then the colours less one, bit 7 the alternative file), and a
; 16-bit frame each (twice as many with a trigger that has a second set;
; none for the rotations): a RAM address in bank $7E or an offset into the
; alternative file, or, for a single colour, the colour itself.
;
; Tables, where Lunar Magic's layout has them: each level's settings byte
; at $03FE00 (PTLG----: the game's colour $64 flashing, the game's animated
; tiles, the level's list, the global list, each off when set); the
; alternative files' 3-byte pointers at $03BCC0; and, in the block the JSL
; at $0583AD leads to, at +$EA the address of 512 3-byte pointers to the
; levels' lists ($0000FF: none, a middle byte of 0), at +$5B the global
; list's bank times $100 (0: none) and at +$65 its low word. Lunar Magic's
; editor reads and writes those three at those offsets of the hook's target.
;
; Each frame the game's animation runs (JSL CODE_05BB39, in the level, the
; level's setup, and the ending), slots k, k+8, k+16, and k+24 of the global
; list and then of the level's run, k being the frame counter $14 mod 8:
; each moves its frame counter ($7FC080 level, $7FC0A0 global, $FF at the
; load) as its trigger says and queues its frame's upload, once for each
; value of $14 (which stands still while the game is stopped), which the NMI
; makes after the game's own animated tiles, the later slot over the
; earlier. $7FC004 counts every eighth frame, mod $20, for older custom
; blocks. Triggers: $7FC070 the manual frames, $7FC0F8 the one-shot bits,
; $7FC0FC the custom bits.
;
; Lunar Magic treats its ExAnimation as installed when $00A390 is a JSL,
; whatever it calls; Kobo's NMI hook is one, so a save keeps this code and
; uses its tables. It keeps the settings table at $03FE00 only when the
; byte before it, $03FDFF, is $00 (any other value, and it sets every
; level's again), so this writes that byte.

incsrc "memory.asm"
!overworld = 0

; ---------------------------------------------------------------------------
; Hooks.

; LoadLevel: SEP #$30 : STZ $1933. Every level's load starts here, the
; level number known ($010B).
org $0583AD
    autoclean JSL level_init
    NOP

; The game's tile animation, JSL CODE_05BB39: in the ending, the level, and
; the level's setup (eight times, with the uploads after each).
org $0095B5
    JSL frame
org $00A2A5
    JSL frame
org $00A5FD
    JSL setup_frame

; The NMI's animated tile upload and colour $64 flashing (CODE_00A390,
; entered with JSR by the NMI and the level's setup).
org $00A390
    JSL nmi
    RTS

; ClearMemory at reset: REP #$30 : LDX #$1FFE.
org $008A4E
    JSL clear_memory
    NOP

freedata

level_table:
    for i = 0..512
        dl $0000FF
    endfor

freecode
prot level_table

incsrc "exanimation-engine.asm"

; ---------------------------------------------------------------------------
; The level's load. Lunar Magic's editor finds the tables through the three
; values at fixed offsets from here.
level_init:
    BRL level_init_body
    fillbyte $FF : fill $5B-3
global_bank:                    ; +$5B
    dw $0000
    fillbyte $FF : fill $65-$5D
global_low:                     ; +$65
    dw $0000
    fillbyte $FF : fill $EA-$67
level_list:                     ; +$EA
    dl level_table

assert global_bank-level_init == $5B
assert global_low-level_init == $65
assert level_list-level_init == $EA

level_init_body:
    SEP #$30
    STZ $1933|!addr             ; what the hook replaces
    PHP
    JSR save_scratch
    REP #$30
    LDA #$FFFF
    LDX #$003E
-   STA.l !Counters,x           ; every slot's counter
    DEX
    DEX
    BPL -
    LDA #$0000
    STA.l !OneShot
    STA.l !OneShot+2
    JSR clear_queue
    SEP #$20
    LDA #$1F                    ; the first eighth frame counts it to 0
    STA.l !Legacy
    LDA #$FF
    STA.l !LastFrame
    STA.l !LastCall
    REP #$20
    ; The triggers' starting states: the global list's, then the level's.
    JSR global_address
    BCC +
    JSR init_triggers
+   JSR level_address
    BCC +
    JSR init_triggers
+   JSR restore_scratch
    PLP
    RTL


; !List = the global list; carry clear for none. A/X/Y 16-bit.
global_address:
    LDA.l global_bank           ; the bank in the high byte
    BEQ .none
    STA !List+1
    LDA.l global_low
    STA !List
    SEC
    RTS
.none:
    CLC
    RTS

; !List = the level's list; carry clear for none. A/X/Y 16-bit.
level_address:
    LDA.l level_list            ; the table, wherever Lunar Magic keeps it
    STA !Source
    LDA.l level_list+1
    STA !Source+1
    LDA.l !Level
    AND #$01FF
    STA !List
    ASL A
    CLC
    ADC !List
    TAY
    LDA [!Source],y
    STA !List
    INY
    LDA [!Source],y
    STA !List+1
    AND #$00FF                  ; the middle byte: 0 for none
    BEQ .none
    SEC
    RTS
.none:
    CLC
    RTS


; ---------------------------------------------------------------------------
; Reset: the triggers and the rest cleared, and what the hook replaces.
clear_memory:
    REP #$30
    LDA #$0000
    LDX #$008E
-   STA.l !Manual,x
    DEX
    DEX
    BPL -
    STA.l !Legacy-1             ; $7FC003-$7FC004
    LDX #$1FFE                  ; what the hook replaces
    RTL

; ---------------------------------------------------------------------------
; A frame of the animation, where the game runs its own. A/X/Y 8-bit.
frame:
    PHP
    JSR save_scratch
    REP #$30
    STZ !Frames                 ; bit 0: a list with slots runs
    JSR settings
    AND #$10
    BNE +
    REP #$20
    JSR global_address
    BCC +
    SEP #$20
    LDA [!List]
    BEQ +
    LDA #$01
    TSB !Frames
+   JSR settings
    AND #$20
    BNE +
    REP #$20
    JSR level_address
    BCC +
    SEP #$20
    LDA [!List]
    BEQ +
    LDA #$01
    TSB !Frames
+   SEP #$20
    LDA !EffFrame
    CMP.l !LastCall
    BNE +
    ; Run for this $14 already (the game stopped it): the game's tiles too,
    ; and the NMI has nothing to upload again, so that the palette the game
    ; uploads before it (a fade) stays.
    REP #$20
    JSR clear_queue
    BRA .done
+   STA.l !LastCall
    JSR settings
    AND #$40
    BNE +
    SEP #$30
    JSL $05BB39                 ; the game's animated tiles
+   SEP #$20
    REP #$10
    LDA !Frames
    BEQ .done                   ; neither list
    LDA !EffFrame
    STA.l !LastFrame
    REP #$20
    JSR clear_queue
    SEP #$20
    LDA !EffFrame
    AND #$07
    BNE +
    LDA.l !Legacy               ; every eighth frame a list runs
    INC A
    AND #$1F
    STA.l !Legacy
+   JSR settings
    AND #$10
    BNE +
    REP #$20
    JSR global_address
    BCC +
    SEP #$20
    LDA #$20
    STA !Counter
    LDA #$00
    JSR run_list
+   JSR settings
    AND #$20
    BNE .done
    REP #$20
    JSR level_address
    BCC .done
    SEP #$20
    STZ !Counter
    LDA #$1C                    ; the level's uploads from the fifth on
    JSR run_list
.done:
    JSR restore_scratch
    PLP
    RTL

; The level's setup runs the animation eight times, from $14 mod 8 = 0,
; before it copies the palette to the copy the game fades (MVN at $00A5E1);
; the rotations and the working palettes take that copy as theirs, so it
; is made before the first time as well.
setup_frame:
    PHP
    SEP #$20
    LDA !EffFrame
    AND #$07
    BNE +
    REP #$30
    PHB
    LDX.w #!MainPalette&$FFFF
    LDY.w #!CopyPalette&$FFFF
    LDA #$01FF                  ; all 256 colours (the game's copy stops at $F7)
    MVN $00,$00                 ; bank $00: either RAM, LoROM or SA-1
    PLB
+   PLP
    JML frame

; A = the level's settings byte; A 8-bit and X/Y 16-bit after.
settings:
    REP #$30
    LDA.l !Level
    AND #$01FF
    TAX
    SEP #$20
    LDA.l !Settings,x
    RTS
; ---------------------------------------------------------------------------
; The uploads, in the NMI and in the level's setup: the game's animated
; tiles, the queue, and the game's colour $64 flashing, as the level's
; settings allow.
nmi:
    PHP
    REP #$30
    LDA.l !Level
    AND #$01FF
    TAX
    SEP #$20
    LDA.l !Settings,x
    PHA
    AND #$40
    BNE +
    JSR game_tiles
+   JSR queue
    SEP #$20
    PLA
    AND #$80
    BNE +
    JSR game_flashing
+   PLP
    RTL

; The game's animated tiles ($0D76-$0D81): three tiles of four, the one at
; $0800 in two halves, the second at $0900.
game_tiles:
    REP #$20
    SEP #$10
    LDY #$80
    STY $2115
    LDA #$1801
    STA $4320
    LDY #$7E
    STY $4324
    LDX #$04
    LDA $0D80|!addr
    BEQ +
    STA $2116
    LDA $0D7A|!addr
    STA $4322
    LDA #$0080
    STA $4325
    STX $420B
+   LDA $0D7E|!addr
    BEQ +
    STA $2116
    LDA $0D78|!addr
    STA $4322
    LDA #$0080
    STA $4325
    STX $420B
+   LDA $0D7C|!addr
    BEQ .done
    STA $2116
    CMP #$0800
    BEQ .halves
    LDA $0D76|!addr
    STA $4322
    LDA #$0080
    STA $4325
    STX $420B
    BRA .done
.halves:
    LDA $0D76|!addr
    STA $4322
    LDA #$0040
    STA $4325
    STX $420B
    LDA #$0900
    STA $2116
    LDA $0D76|!addr
    CLC
    ADC #$0040
    STA $4322
    LDA #$0040
    STA $4325
    STX $420B
.done:
    SEP #$20
    REP #$10
    RTS

; Colour $64 from the game's flashing colours, by $14.
game_flashing:
    SEP #$30
    LDA #$64
    STZ $00
    STA $2121
    LDA !EffFrame
    AND #$1C
    LSR A
    TAX
    LDA.l $00B60C,x             ; FlashingColors
    STA $2122
    LDA.l $00B60D,x
    STA $2122
    REP #$10
    RTS

; ---------------------------------------------------------------------------
; The alternative files: none until a build writes them.
org !AltFiles
    dl $000000,$000000,$000000,$000000

; Every level's settings as Lunar Magic's first save leaves them (the game
; has $FF there, everything off): all on, but level 104's lists (the
; ending's Yoshi's House), until a build writes them.
org !Settings-1
    db $00                      ; Lunar Magic keeps the table only with $00 here
    fillbyte $00 : fill $104
    db $30
    fillbyte $00 : fill $FB
