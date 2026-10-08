; Lunar Magic's overworld ExAnimation: each submap's animated tiles and
; colours, in the format of a level's list (exanimation-engine.asm), with a
; settings byte a submap.
;
; Kobo's own code, written from the vanilla code it hooks (SMWDisX), Lunar
; Magic's help ("Edit Submap ExAnimated Frames", "Edit Animation
; Settings"), and what Lunar Magic-saved ROMs' overworlds do with their
; lists frame by frame, observed as memory effects only
; (docs/lunar-magic-install.md, "The overworld"); never from Lunar Magic's
; code.
;
; Tables, where Lunar Magic's layout has them, at fixed offsets from the
; block the JSL at $048086 leads to: at +$4A the address of 7 settings
; bytes, one a submap (bits 7 to 4: the level dots' flashing colours, the
; game's animated tiles, the submap's list, the global list, each off when
; set), at +$57 the global list's bank times $100 (0: none) and at +$61
; its low word, and at +$E1 the address of 7 3-byte pointers to the
; submaps' lists ($0000FF: none, a middle byte of 0). The header's other
; bytes are 0.
;
; The lists run as a level's do, the overworld's global list in the level
; global list's place and the player's submap's in the level's: counters at
; $7FC0A0 and $7FC080, the submap's uploads after the global ones'. On the
; overworld $7FC004 is not kept.

incsrc "memory.asm"
!overworld = 1

!PlayerTurn = $0DB3|!addr       ; the player whose turn it is
!PlayerSubmaps = $1F11|!addr
!OverworldAnimation = $0AF6|!addr ; GfxDecompOWAni: the game's animated tiles

; ---------------------------------------------------------------------------
; Hooks.

; CODE_048086, the game's animated tiles set up: REP #$30 : STZ $03 : STZ
; $05. The overworld's load, a submap change, and a player switch run it.
org $048086
    autoclean JSL init
    BRA init_return
org $04808C
init_return:

; OW_Tile_Animation, each frame: LDA $13 : AND #$07 : BNE, whose BNE stays,
; taking $14's: the frame runs the lists first.
org $0480E0
    JSL frame
org $0480E4
    db $D0                      ; BNE, as the game has it

; The waterfall's and the rest's LDA $13 in OW_Tile_Animation: $14, as the
; first check now is, so that a stopped game stops them all (Lunar Magic's
; help: "the bottom waterfall tile animation would not consistently match up
; with the main waterfall tile").
org $048102 : db $14
org $04810D : db $14
org $04813B : db $14

; CODE_00A4E3, in the NMI: REP #$10 : LDA #$80 : STA $2115, the start of the
; game's upload of its animated tiles, which the JSL makes, with the queue,
; and goes on in the game's code itself. Lunar Magic takes its overworld
; ExAnimation as installed by a JSL here.
org $00A4E3
    JSL nmi

freedata

settings_table:
    db $00,$00,$00,$00,$00,$00,$00
submap_table:
    for i = 0..7
        dl $0000FF
    endfor

freecode
prot settings_table

incsrc "exanimation-engine.asm"

; ---------------------------------------------------------------------------
; The set up. Lunar Magic's editor finds the tables through the values at
; fixed offsets from here.
init:
    BRL init_body
    fillbyte $00 : fill $4A-3
settings_pointer:               ; +$4A
    dl settings_table
    fill $57-$4D
global_bank:                    ; +$57
    dw $0000
    fill $61-$59
global_low:                     ; +$61
    dw $0000
    fill $E1-$63
submap_pointer:                 ; +$E1
    dl submap_table

assert settings_pointer-init == $4A
assert global_bank-init == $57
assert global_low-init == $61
assert submap_pointer-init == $E1

init_body:
    PHP
    SEP #$20
    PHA                         ; room for the settings, under the scratch
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
    LDA #$FF
    STA.l !LastFrame
    STA.l !LastCall
    REP #$20
    ; The triggers' starting states, whatever the settings: the global
    ; list's, then the submap's.
    JSR global_address
    BCC +
    JSR init_triggers
+   JSR submap_address
    BCC +
    JSR init_triggers
+   ; The first frames: the lists for $14 = 0 to 7, which leaves $14 at 8,
    ; so that every slot has shown its first frame. In the load the screen
    ; is off, and each frame's uploads are made at once; otherwise the NMI
    ; makes the last frame's.
    SEP #$30
    STZ !EffFrame
-   JSR run_frame
    SEP #$20
    LDA $0100|!addr
    CMP #$0C
    BNE +
    JSR queue
+   SEP #$30
    INC !EffFrame
    LDA !EffFrame
    CMP #$08
    BNE -
    JSR settings
    STA $11,s                   ; the room, under the 16 bytes of scratch
    JSR restore_scratch
    SEP #$20
    PLA
    AND #$40
    BNE .no_game
    PLP
    REP #$30                    ; what the hook replaces
    STZ $03
    STZ $05
    RTL
.no_game:
    ; With the game's animated tiles off, their set up is skipped too: to
    ; the SEP #$30 : RTS that ends CODE_048086.
    PLP
    SEP #$20
    PLA
    PLA
    PLA
    JML $0480B6

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

; !List = the player's submap's list; carry clear for none. A/X/Y 16-bit.
submap_address:
    LDA.l submap_pointer
    STA !Source
    LDA.l submap_pointer+1
    STA !Source+1
    JSR submap
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

; A (16-bit) = the player's submap. X/Y 16-bit.
submap:
    LDA !PlayerTurn
    AND #$00FF
    TAX
    LDA !PlayerSubmaps,x
    AND #$00FF
    RTS

; A = the player's submap's settings byte; A 8-bit and X/Y 16-bit after.
settings:
    REP #$30
    LDA.l settings_pointer
    STA !Source
    LDA.l settings_pointer+1
    STA !Source+1
    JSR submap
    TAY
    SEP #$20
    LDA [!Source],y
    RTS

; ---------------------------------------------------------------------------
; A frame, from OW_Tile_Animation's start (A/X/Y 8-bit): the lists, then
; the game's animated tiles unless the settings turn them off. Returns with
; $14 AND 7 in A for the game's BNE.
frame:
    PHP
    SEP #$20
    PHA                         ; room for the settings, under the scratch
    JSR save_scratch
    JSR run_frame
    JSR settings
    STA $11,s                   ; the room, under the 16 bytes of scratch
    JSR restore_scratch
    SEP #$20
    PLA
    PLP
    AND #$40
    BNE .no_game
    LDA !EffFrame
    AND #$07
    RTL
.no_game:
    ; Past the game's animated tiles: to the RTS of CODE_048086 (after its
    ; SEP #$30), which returns from OW_Tile_Animation.
    PLA
    PLA
    PLA
    JML $0480B6

; The lists' frame for $14, once for each value of it. Scratch saved.
run_frame:
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
    JSR submap_address
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
    REP #$20                    ; run for this $14 already
    JSR clear_queue
    BRA .done
+   STA.l !LastCall
    LDA !Frames
    BEQ .done                   ; no list
    LDA !EffFrame
    STA.l !LastFrame
    REP #$20
    JSR clear_queue
    JSR settings
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
    JSR submap_address
    BCC .done
    SEP #$20
    STZ !Counter
    LDA #$1C                    ; the submap's uploads from the fifth on
    JSR run_list
.done:
    RTS

; ---------------------------------------------------------------------------
; The NMI's upload, from CODE_00A4E3's start (A 8-bit): the game's animated
; tiles, to where the game's own upload sends them (its LDY #$0750, which
; an option that moves FG1 and FG2, such as Lunar Magic's merging them
; into SP3 and SP4, changes), then the queue, then the game's level dot
; colours, as the settings allow, during a submap change too. The settings are read through $00-$02, the scratch the
; game's NMI uses.
nmi:
    REP #$30
    LDA.l settings_pointer
    STA $00
    LDA.l settings_pointer+1
    STA $01
    JSR submap
    TAY
    SEP #$20
    LDA [$00],y
    PHA
    AND #$40
    BNE +
    LDA #$80
    STA $2115
    REP #$20
    LDA.l $00A4EB               ; the game's LDY #$0750: where they go
    TAY
    SEP #$20
    STY $2116
    LDY #$1801
    STY $4320
    LDY.w #!OverworldAnimation
    STY $4322
    STZ $4324
    LDY #$0160
    STY $4325
    LDA #$04
    STA $420B
+   PHP
    JSR queue
    PLP
    PLA
    AND #$80
    BNE .no_dots
    ; The dots' colours, past the game's check for a submap change in
    ; progress, as Lunar Magic's upload makes them then too: to the game's
    ; LDA #$6D after it, which returns from CODE_00A4E3.
    PLA
    PLA
    PLA
    SEP #$30
    JML $00A513
.no_dots:
    PLA                         ; to the game's RTS just before, with X/Y
    PLA                         ; 8-bit as its own way out leaves them
    PLA
    SEP #$30
    JML $00A4E2
