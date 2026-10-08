; ExAnimation's engine, shared by Kobo's code for levels (exanimation.asm)
; and for the overworld (overworld-exanimation.asm), each of which includes
; it after memory.asm and sets !overworld (0 or 1) first: running a list's
; slots, their triggers and counters, the uploads they queue, and the NMI's
; upload of the queue. Kobo's own code, as exanimation.asm says.
;
; The data format and the RAM are Lunar Magic's, as its help documents them
; for both ("Edit Level ExAnimated Frames", "Edit Submap ExAnimated
; Frames"). On the overworld, triggers 01-08 are the events named by manual
; frames 8-F, each with a second set, and types 15 and 17 do not stop at a
; level's end.

!Settings = $03FE00
!AltFiles = $03BCC0
!Level = !addr|$00010B
!EffFrame = $14
!EndLevelTimer = !addr|$001493
!BackAreaColour = !addr|$000701
!MainPalette = !addr|$000703
!CopyBackArea = !addr|$000903
!CopyPalette = !addr|$000905
; The copy colour uploads are made from: in a level the one the game fades,
; on the overworld, which keeps no such copy, the palette itself.
if !overworld
    !UploadPalette = !MainPalette
else
    !UploadPalette = !CopyPalette
endif

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
if !overworld == 0
    LDA.l !EndLevelTimer        ; types 15 and 17 stop at the level's end
    BEQ .trigger
    RTS
endif
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
if !overworld == 0
    CMP #$06
    BEQ .condition_once
    CMP #$08
    BEQ .condition_once
endif
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
; On the overworld, 01-08 are events: each holds once the event in manual
; frame 8-F (01 the eighth's) is passed.
condition:
    CMP #$20
    BCS .custom
if !overworld
    CMP #$01
    BCC +
    CMP #$09
    BCS +
    JMP .event
+
endif
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
if !overworld
.event:
    CLC
    ADC #$07                    ; manual frame 8-F
    REP #$20
    AND #$00FF
    TAX
    SEP #$20
    LDA.l !Manual,x             ; the event
    PHA
    AND #$07
    REP #$20
    AND #$00FF
    TAX
    SEP #$20
    LDA.l .event_bits,x
    STA !Temp
    PLA
    LSR A
    LSR A
    LSR A
    REP #$20
    AND #$00FF
    TAX
    SEP #$20
    LDA.l $001F02|!addr,x       ; OWEventsActivated, the highest bit first
    AND !Temp
    CMP #$01
    RTS
.event_bits:
    db $80,$40,$20,$10,$08,$04,$02,$01
endif

; Carry set when trigger A holds for a rotation: the game's conditions with
; a second set (01-05, 07), 09-0F always, and the custom bits; 06, 08, the
; manual and one-shot triggers never. On the overworld 01-08 are events,
; which hold as the rest do.
turn_holds:
if !overworld == 0
    CMP #$06
    BEQ .no
    CMP #$08
    BEQ .no
endif
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
if !overworld == 0
    STA.l !CopyPalette,x
endif
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
    ADC.w #!UploadPalette&$FFFF
    STA !Source
    SEP #$20
    LDA.b #!UploadPalette>>16
    STA !Source+2
    BRA colours_queue
.back_area:
    REP #$20
    LDA !Source                 ; the colour itself
    STA.l !BackAreaColour
if !overworld == 0
    STA.l !CopyBackArea
endif
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
    ; Turning anyway: 06 and 08 move while their condition holds (in a
    ; level; on the overworld they are events, which turn as the rest), a
    ; one-shot while its bit is on (and stays on), the rest always.
    LDY !Slot
    INY
    LDA [!List],y
if !overworld == 0
    CMP #$06
    BEQ .while
    CMP #$08
    BEQ .while
endif
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
if !overworld == 0
    LDA.l !CopyPalette,x
    PHA
endif
    DEY
    BEQ +
-   LDA.l !MainPalette-2,x
    STA.l !MainPalette,x
if !overworld == 0
    LDA.l !CopyPalette-2,x
    STA.l !CopyPalette,x
endif
    DEX
    DEX
    DEY
    BNE -
+   BRA .ends
.left:
    LDA.l !MainPalette,x
    PHA
if !overworld == 0
    LDA.l !CopyPalette,x
    PHA
endif
    DEY
    BEQ +
-   LDA.l !MainPalette+2,x
    STA.l !MainPalette,x
if !overworld == 0
    LDA.l !CopyPalette+2,x
    STA.l !CopyPalette,x
endif
    INX
    INX
    DEY
    BNE -
+
.ends:
if !overworld == 0
    PLA
    STA.l !CopyPalette,x
endif
    PLA
    STA.l !MainPalette,x
    SEP #$20
    JSR rotate_range
    STX !Source
    REP #$20
    LDA !Source
    CLC
    ADC.w #!UploadPalette&$FFFF
    STA !Source
    SEP #$20
    LDA.b #!UploadPalette>>16
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
