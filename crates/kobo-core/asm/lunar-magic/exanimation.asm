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

!Settings = $03FE00
!AltFiles = $03BCC0
!Level = !addr|$00010B
!EffFrame = $14
!EndLevelTimer = !addr|$001493
!BackAreaColour = !addr|$000701
!MainPalette = !addr|$000703
!CopyBackArea = !addr|$000903
!CopyPalette = !addr|$000905

!Legacy = $7FC004
!LastCall = $7FC003             ; the $14 of the last frame
!LastFrame = $7FC019            ; the $14 the lists last ran for
!Manual = $7FC070
!Counters = $7FC080             ; level slots, the global ones $20 on
!Queue = $7FC0C0                ; 8 uploads of 7 bytes (see queue)
!OneShot = $7FC0F8
!Custom = $7FC0FC

; Direct page scratch, free where the game's animation runs (it uses $00),
; and in the NMI's uploads (which use $00).
!List = $00                     ; the list's address, 3 bytes
!Source = $03                   ; an upload's source, 3 bytes
!Base = $06                     ; 16-bit: the slot offsets, from !List
!Slot = $08                     ; 16-bit: the slot's entry, from !List
!Index = $0A                    ; the slot's number
!Counter = $0B                  ; its counter's index from !Counters
!Entry = $0C                    ; its upload's offset from !Queue
!Frames = $0D                   ; its frames (the frames less one, plus one)
!Temp = $0E                     ; 16-bit

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

freedata

level_table:
    for i = 0..512
        dl $0000FF
    endfor

freecode
prot level_table

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

; The direct page scratch this code uses, $00-$0F, kept for the game's code
; around the hooks, which leaves values there across them. The caller's
; return address stays on top of the stack.
save_scratch:
    REP #$30
    PLA                         ; the return address
    PEI ($00)
    PEI ($02)
    PEI ($04)
    PEI ($06)
    PEI ($08)
    PEI ($0A)
    PEI ($0C)
    PEI ($0E)
    PHA
    RTS

restore_scratch:
    REP #$30
    PLA
    PLX
    STX $0E
    PLX
    STX $0C
    PLX
    STX $0A
    PLX
    STX $08
    PLX
    STX $06
    PLX
    STX $04
    PLX
    STX $02
    PLX
    STX $00
    PHA
    RTS

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

; The custom triggers the list sets, and the manual ones. A/X/Y 16-bit.
init_triggers:
    LDY #$0002
    LDA [!List],y               ; those it keeps
    AND.l !Custom
    LDY #$0004
    ORA [!List],y               ; and those it sets
    STA.l !Custom
    LDY #$0006
    LDA [!List],y
    STA !Temp                   ; the manual triggers it sets
    LDY #$0008
    LDX #$0000
-   LSR !Temp
    BCC +
    SEP #$20
    LDA [!List],y
    STA.l !Manual,x
    REP #$20
    INY
+   INX
    CPX #$0010
    BNE -
    RTS

clear_queue:                    ; A 16-bit
    LDA #$0000
    STA.l !Queue
    STA.l !Queue+7
    STA.l !Queue+14
    STA.l !Queue+21
    STA.l !Queue+28
    STA.l !Queue+35
    STA.l !Queue+42
    STA.l !Queue+49
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

; The list at !List, from slot $14 mod 8 in steps of 8. A = the first
; upload's offset. A 8-bit, X/Y 16-bit.
run_list:
    STA !Entry
    REP #$20
    LDY #$0006
    LDA [!List],y               ; the manual triggers set: a frame each
    LDX #$0008
-   LSR A
    BCC +
    INX
+   CMP #$0000
    BNE -
    STX !Base
    SEP #$20
    LDA !EffFrame
    AND #$07
    STA !Index
.slot:
    LDA [!List]                 ; slots used
    CMP !Index
    BEQ .end
    BCC .end
    REP #$20
    LDA !Index
    AND #$00FF
    ASL A
    CLC
    ADC !Base
    TAY
    LDA [!List],y
    BEQ +                       ; unused
    CLC
    ADC !Base
    STA !Slot
    SEP #$20
    JSR run_slot
+   SEP #$20
    LDA !Entry
    CLC
    ADC #$07
    STA !Entry
    LDA !Index
    CLC
    ADC #$08
    STA !Index
    CMP #$20
    BCC .slot
.end:
    RTS

; ---------------------------------------------------------------------------
; A slot. A 8-bit, X/Y 16-bit; !Slot its entry.
run_slot:
    LDY !Slot
    INY
    INY
    LDA [!List],y               ; frames less one
    INC A
    STA !Frames
    LDY !Slot
    LDA [!List],y               ; the type
    CMP #$18
    BCC +
    JMP rotation
+   CMP #$15
    BEQ .fades
    CMP #$17
    BNE .trigger
.fades:
    LDA.l !EndLevelTimer        ; types 15 and 17 stop at the level's end
    BEQ .trigger
    RTS
.trigger:
    INY
    LDA [!List],y               ; the trigger
    CMP #$10
    BCC .game
    CMP #$20
    BCC .manual
    CMP #$30
    BCC .custom
    CMP #$50
    BCC .one_shot
    BRA .plain
.game:
    CMP #$06
    BEQ .condition_once
    CMP #$08
    BEQ .condition_once
    CMP #$0F
    BEQ .plain                  ; the precision timer: no second set
.custom:
    JSR condition
    BCS .second
.plain:
    JSR advance
    JMP upload
.second:
    JSR advance
    CLC
    ADC !Frames
    JMP upload
.condition_once:
    JSR condition
    BCC .none
    JSR counter
    INC A
    CMP !Frames
    BCS .none                   ; ran once: stays on its last frame
    STA.l !Counters,x
    JMP upload
.none:
    RTS
.manual:
    AND #$0F
    REP #$20
    AND #$00FF
    TAX
    SEP #$20
    LDA.l !Manual,x
    STA !Temp
    JSR counter
    LDA !Temp
    CMP.l !Counters,x
    BEQ .none                   ; shown already ($FF: nothing yet)
    CMP !Frames
    BCC +
    LDA #$00                    ; past the last frame: the first, again
+   STA.l !Counters,x           ; each time, as the two stay unequal
    JMP upload
.one_shot:
    SEC
    SBC #$30
    JSR one_shot_bit
    BEQ .none                   ; off: the animation holds
    JSR counter
    INC A
    CMP !Frames
    BCS .finished
    STA.l !Counters,x
    JMP upload
.finished:                      ; done: the counter back to $FF, the bit off
    LDA #$FF
    STA.l !Counters,x
    LDY !Slot
    INY
    LDA [!List],y
    SEC
    SBC #$30
    JSR one_shot_bit
    LDA !Temp
    EOR #$FF
    AND.l !OneShot,x
    STA.l !OneShot,x
    RTS

; X = the slot's counter's index; A = the counter. Trigger 0F (the help's
; "precision timer") shares the counter of the first slot of its group of
; eight. Y kept.
counter:
    PHY
    LDY !Slot
    INY
    LDA [!List],y               ; the trigger
    PLY
    CMP #$0F
    BNE +
    LDA !Index
    AND #$F8
    BRA ++
+   LDA !Index
++  CLC
    ADC !Counter
    REP #$20
    AND #$00FF
    TAX
    SEP #$20
    LDA.l !Counters,x
    RTS

; The counter moved on, from 0 again past the last frame; A = it.
advance:
    JSR counter
    INC A
    CMP !Frames
    BCC +
    LDA #$00
+   STA.l !Counters,x
    RTS

; One-shot trigger A: X = its byte's index, !Temp = its bit, Z set when off.
one_shot_bit:
    PHA
    AND #$07
    REP #$20
    AND #$00FF
    TAX
    SEP #$20
    LDA.l .bits,x
    STA !Temp
    PLA
    LSR A
    LSR A
    LSR A
    REP #$20
    AND #$00FF
    TAX
    SEP #$20
    LDA.l !OneShot,x
    AND !Temp
    RTS
.bits:
    db $01,$02,$04,$08,$10,$20,$40,$80

; Carry set when trigger A (the game's conditions, or a custom bit) holds.
condition:
    CMP #$20
    BCS .custom
    CMP #$01
    BEQ .pow
    CMP #$02
    BEQ .silver
    CMP #$03
    BEQ .on_off
    CMP #$04
    BEQ .star
    CMP #$05
    BEQ .timer
    CMP #$06
    BEQ .timer
    CMP #$07
    BEQ .coins
    CMP #$08
    BEQ .coins
    CMP #$10
    BCS +
    CMP #$09
    BCS .holds                  ; 09-0F, reserved: always
+   CLC
    RTS
.pow:
    LDA.l $0014AD|!addr
    BRA .nonzero
.silver:
    LDA.l $0014AE|!addr
    BRA .nonzero
.on_off:
    LDA.l $0014AF|!addr
    BRA .nonzero
.star:
    LDA.l $001490|!addr
.nonzero:
    CMP #$01
    RTS
.timer:
    LDA.l $000F31|!addr         ; the timer's hundreds: none left
    BEQ .holds
    CLC
    RTS
.holds:
    SEC
    RTS
.coins:
    LDA.l $001420|!addr
    CMP #$05
    RTS
.custom:
    SEC
    SBC #$20
    REP #$20
    AND #$000F
    TAX
    LDA.l !Custom
-   DEX
    BMI +
    LSR A
    BRA -
+   LSR A
    SEP #$20
    RTS

; Carry set when trigger A holds for a rotation: the game's conditions with
; a second set (01-05, 07), 09-0F always, and the custom bits; 06, 08, the
; manual and one-shot triggers never.
turn_holds:
    CMP #$06
    BEQ .no
    CMP #$08
    BEQ .no
    CMP #$10
    BCC .condition
    CMP #$20
    BCC .no
    CMP #$30
    BCS .no
.condition:
    JMP condition
.no:
    CLC
    RTS

; ---------------------------------------------------------------------------
; The upload of frame A of the slot. A 8-bit, X/Y 16-bit.
upload:
    REP #$20
    AND #$00FF
    ASL A
    CLC
    ADC !Slot
    ADC #$0005
    TAY                         ; the frame's word
    CLC
    ADC !List
    STA !Temp                   ; its address
    LDA [!List],y
    STA !Source
    SEP #$20
    LDY !Slot
    LDA [!List],y               ; the type
    CMP #$13
    BCC tiles
    JMP colours

; Tiles: the source in bank $7E, or in the alternative file.
tiles:
    REP #$20
    AND #$00FF
    ASL A
    TAX
    LDA.l tile_bytes-2,x
    PHA
    LDA !Entry
    AND #$00FF
    TAX
    PLA
    STA.l !Queue,x
    INY
    INY
    INY
    LDA [!List],y               ; the destination
    PHA
    AND #$7FFF
    STA.l !Queue+2,x
    PLA
    BPL .ram
    JSR alternative
    BRA .queue
.ram:
    SEP #$20
    LDA #$7E
    STA !Source+2
.queue:
    REP #$20
    LDA !Source
    STA.l !Queue+4,x
    SEP #$20
    LDA !Source+2
    STA.l !Queue+6,x
    RTS

; Bytes per upload by type (01-12); bit 14: two rows, this many bytes each,
; the second $100 words below.
tile_bytes:
    dw $0020,$0040,$0060,$0080,$00A0,$00C0,$00E0,$0100
    dw $0180,$0200,$0280,$0300,$0380,$0400
    dw $0010,$4020,$4040,$4080

; !Source (16-bit) + the list's alternative file's address. X kept.
alternative:
    PHX
    SEP #$20
    LDY #$0001
    LDA [!List],y               ; the file, 0-3
    AND #$03
    STA !Source+2
    ASL A
    CLC
    ADC !Source+2
    REP #$20
    AND #$00FF
    TAX
    LDA.l !AltFiles,x
    CLC
    ADC !Source
    STA !Source
    SEP #$20
    LDA.l !AltFiles+2,x
    STA !Source+2
    PLX
    RTS

; Colours: types 13 (CGRAM), 14 and 15 (also the palette's two copies in
; RAM), 16 and 17 (the back area colour and its copy, in RAM). One colour
; is the frame itself; more are read from the frame's address.
colours:
    CMP #$16
    BCS .back_area
    PHA
    INY
    INY
    INY
    INY
    LDA [!List],y
    AND #$7F
    INC A
    STA !Frames                 ; colours
    CMP #$01
    BNE .many
    REP #$20                    ; one: the frame's own word, in the list
    LDA !Temp
    STA !Source
    SEP #$20
    LDA !List+2
    STA !Source+2
    BRA .where
.many:
    LDA [!List],y
    BMI .alternative
    LDA #$7E
    STA !Source+2
    BRA .where
.alternative:
    JSR alternative
.where:
    LDY !Slot
    INY
    INY
    INY
    LDA [!List],y
    STA !Temp                   ; the first colour
    PLA
    CMP #$13
    BEQ colours_queue
    ; The palette's two copies in RAM.
    REP #$20
    LDA !Temp
    AND #$00FF
    ASL A
    TAX
    LDA !Frames
    AND #$00FF
    PHA
    LDY #$0000
-   LDA [!Source],y
    STA.l !MainPalette,x
    STA.l !CopyPalette,x
    INX
    INX
    INY
    INY
    LDA 1,s
    DEC A
    STA 1,s
    BNE -
    PLA
    LDA !Temp                   ; the colours go up from the copy the game fades
    AND #$00FF
    ASL A
    CLC
    ADC.w #!CopyPalette&$FFFF
    STA !Source
    SEP #$20
    LDA.b #!CopyPalette>>16
    STA !Source+2
    BRA colours_queue
.back_area:
    REP #$20
    LDA !Source                 ; the colour itself
    STA.l !BackAreaColour
    STA.l !CopyBackArea
    SEP #$20
    RTS

; A CGRAM upload: !Frames colours to colour !Temp from !Source.
colours_queue:
    REP #$20
    LDA !Entry
    AND #$00FF
    TAX
    LDA !Frames
    AND #$00FF
    ASL A
    ORA #$8000                  ; bit 15: CGRAM
    STA.l !Queue,x
    LDA !Temp
    AND #$00FF
    STA.l !Queue+2,x
    LDA !Source
    STA.l !Queue+4,x
    SEP #$20
    LDA !Source+2
    STA.l !Queue+6,x
    RTS

; ---------------------------------------------------------------------------
; Types 18-1B: the palette's colours rotated in both copies, right (18, 19)
; or left (1A, 1B), once every frames times (the counter counts up to the
; frames less one from $FF). 18 and 1A wait for their trigger, if they have
; one, and 19 and 1B turn the other way while it holds, where a trigger
; holds as turn_holds says; 19 and 1B with a trigger that does not hold
; that way still move as its counter would (06, 08: while the condition
; holds; one-shots: while the bit is on). A manual trigger does nothing
; while its value equals the counter, and otherwise sets the counter to the
; value less one first. As each was seen to run in a Lunar Magic-saved ROM
; (exanim_probe; docs/lunar-magic-install.md, "ExAnimation"). The colours go up from the
; copy the game fades. A 8-bit, X/Y 16-bit.
rotation:
    PHA                         ; the type
    INY
    LDA [!List],y               ; the trigger
    CMP #$10
    BCC .holds_check
    CMP #$20
    BCS .holds_check
    ; Manual: nothing while the value equals the counter; otherwise the
    ; counter at the value less one first.
    AND #$0F
    REP #$20
    AND #$00FF
    TAX
    SEP #$20
    LDA.l !Manual,x
    STA !Temp
    JSR counter
    CMP !Temp
    BNE +
    PLA
    RTS
+   LDA !Temp
    DEC A
    STA.l !Counters,x
    LDY !Slot
    INY
    LDA [!List],y
.holds_check:
    PHA                         ; the trigger
    JSR turn_holds
    PLA
    BCS .holds
    CMP #$00
    BEQ .go                     ; none: always
    LDA 1,s
    CMP #$19
    BEQ .step
    CMP #$1B
    BEQ .step
    PLA                         ; waits for its trigger
    RTS
.holds:
    LDA 1,s
    CMP #$19
    BEQ .reverse
    CMP #$1B
    BNE .go
.reverse:
    EOR #$02                    ; the other way
    STA 1,s
    BRA .go
.step:
    ; Turning anyway: 06 and 08 move while their condition holds, a
    ; one-shot while its bit is on (and stays on), the rest always.
    LDY !Slot
    INY
    LDA [!List],y
    CMP #$06
    BEQ .while
    CMP #$08
    BEQ .while
    CMP #$30
    BCC .go
    CMP #$50
    BCS .go
    SEC
    SBC #$30
    JSR one_shot_bit
    BNE .go
    PLA
    RTS
.while:
    JSR condition
    BCS .go
    PLA
    RTS
.go:
    JSR counter                 ; a turn when it reaches the frames less one
    INC A
    STA !Temp
    LDA !Frames
    DEC A
    CMP !Temp
    BEQ .turn
    BCC .turn                   ; or passes it (a manual value can put it there)
    LDA !Temp
    STA.l !Counters,x
    PLA
    RTS
.turn:
    LDA #$FF
    STA.l !Counters,x
    JSR rotate_range            ; X = 2 * the first colour, Y = the colours
    PLA
    CMP #$1A
    REP #$20
    BCS .left
    TYA                         ; right: from the last colour down
    DEC A
    ASL A
    STA !Source
    TXA
    CLC
    ADC !Source
    TAX
    LDA.l !MainPalette,x
    PHA
    LDA.l !CopyPalette,x
    PHA
    DEY
    BEQ +
-   LDA.l !MainPalette-2,x
    STA.l !MainPalette,x
    LDA.l !CopyPalette-2,x
    STA.l !CopyPalette,x
    DEX
    DEX
    DEY
    BNE -
+   BRA .ends
.left:
    LDA.l !MainPalette,x
    PHA
    LDA.l !CopyPalette,x
    PHA
    DEY
    BEQ +
-   LDA.l !MainPalette+2,x
    STA.l !MainPalette,x
    LDA.l !CopyPalette+2,x
    STA.l !CopyPalette,x
    INX
    INX
    DEY
    BNE -
+
.ends:
    PLA
    STA.l !CopyPalette,x
    PLA
    STA.l !MainPalette,x
    SEP #$20
    JSR rotate_range
    STX !Source
    REP #$20
    LDA !Source
    CLC
    ADC.w #!CopyPalette&$FFFF
    STA !Source
    SEP #$20
    LDA.b #!CopyPalette>>16
    STA !Source+2
    TYA
    STA !Frames
    JMP colours_queue

; X = 2 * the slot's first colour, !Temp = it, Y = its colours. A 8-bit.
rotate_range:
    LDY !Slot
    INY
    INY
    INY
    LDA [!List],y
    STA !Temp
    INY
    LDA [!List],y
    AND #$7F
    INC A
    REP #$20
    AND #$00FF
    TAY
    LDA !Temp
    AND #$00FF
    ASL A
    TAX
    SEP #$20
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

; The queued uploads: 7 bytes each, a 16-bit byte count (0 for none; bit
; 15 CGRAM, the destination a colour; bit 14 two rows, the second $100 words
; on), the destination, and the source. They stay queued until the next
; frame of the animation replaces them, so an NMI without one (the one
; after the level's setup, which uploads the palette in between, or a
; paused frame's) makes them again, as it does the game's animated tiles.
queue:
    REP #$30
    LDX #$0000
.next:
    LDA.l !Queue,x
    BNE +
    JMP .skip
+   BMI .cgram
    AND #$3FFF
    STA $4325
    LDA.l !Queue+2,x
    STA $2116
    LDA.l !Queue+4,x
    STA $4322
    LDA.l !Queue+5,x
    STA $4323                   ; and the bank at $4324
    LDA #$1801
    STA $4320
    SEP #$20
    LDA #$80
    STA $2115
    LDA #$04
    STA $420B
    REP #$20
    LDA.l !Queue,x
    AND #$4000
    BEQ .skip
    LDA.l !Queue,x              ; the second row
    AND #$3FFF
    STA $4325
    STA $00
    LDA.l !Queue+2,x
    CLC
    ADC #$0100
    STA $2116
    LDA.l !Queue+4,x
    CLC
    ADC $00
    STA $4322
    SEP #$20
    LDA #$04
    STA $420B
    REP #$20
    BRA .skip
.cgram:
    AND #$3FFF
    STA $4325
    LDA.l !Queue+4,x
    STA $4322
    LDA.l !Queue+5,x
    STA $4323
    LDA #$2200
    STA $4320
    SEP #$20
    LDA.l !Queue+2,x
    STA $2121
    LDA #$04
    STA $420B
    REP #$20
.skip:
    TXA
    CLC
    ADC #$0007
    TAX
    CPX #$0038
    BEQ +
    JMP .next
+   RTS
