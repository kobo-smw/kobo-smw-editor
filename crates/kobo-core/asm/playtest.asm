; Play from here: a ROM that goes from the title screen straight into a
; level, at a place the editor chose. On the title screen's first frame
; (game mode $07), it starts a game as the file select does and takes a
; screen exit to secondary entrance !entrance, which the editor adds to
; the level for it (kobo_core::playtest). Where the game would load the
; overworld (after a death, a game over, or the level's end), it enters
; the level there again, with four lives after a game over.
;
; Written from the game's routines as the SMWDisX disassembly documents
; them: GM07TitleScreen ($009C64), the new game LoadSaveAndFadeToOW
; ($009E17) starts, GM0CLoadOverworld ($00A087), whose own path into a
; level sets game mode $10 and ends at Mode04Finish ($0093F7),
; WriteGameModeAndReturn ($009C8B), and the
; screen exit path of the entrance code (docs/smw.md).
;
; Defines: !entrance, the secondary entrance (000-1FF); !powerup, the
; player's power-up (0 small, 1 big, 2 cape, 3 fire); !time, the level's
; time setting (0-3), which an entry into a sublevel does not load.

incsrc "memory.asm"

; The title screen's code begins with two JSRs, which the hook replaces;
; the overworld's load with a JSR to turn the screen off, then a load of
; the star warp flag, which its hook replaces.
assert read1($009C64) == $20 && read1($009C67) == $20, "the title screen's code is not the game's"
assert read1($00A087) == $20 && read1($00A08A) == $AD, "the overworld's loading code is not the game's"

org $009C64
    JML playtest

org $00A08A
    JML playtest_again

freecode

; Game mode $07, the title screen.
playtest:
    SEP #$30
    ; A one-player game with four lives, as the file select starts one.
    STZ $0DB2|!addr                 ; one player
    STZ $0DB3|!addr                 ; Mario's turn
    LDA #$04
    STA $0DB4|!addr                 ; Mario's lives, kept
    STA $0DBE|!addr                 ; and in play
    LDA #$FF
    STA $0DB5|!addr                 ; no Luigi
    STZ $0DBF|!addr                 ; coins
    STZ $0DC1|!addr                 ; no Yoshi
    STZ $0DC2|!addr                 ; nothing in the item box
    STZ $13C9|!addr                 ; no continue prompt
    LDA.b #!powerup
    STA $0019|!dp
    ; The level is no longer the title screen's.
    STZ $0109|!addr
    JSR enter
    ; Fade out of the title screen, then load the level.
    LDY #$0F
    JML $009C8B                     ; STY GameMode : RTS

; Game mode $0C, the overworld's load, once it has turned the screen off:
; after a death, or the level's end, the level again instead.
playtest_again:
    SEP #$30
    ; After a game over, four lives again.
    STZ $13C9|!addr                 ; no continue prompt
    LDA $0DBE|!addr
    BPL +
    LDA #$04
    STA $0DB4|!addr
    STA $0DBE|!addr
+
    JSR enter
    ; The screen is dark already: load the level, as the overworld's
    ; load does for a level it goes into at once (OverworldOverride),
    ; which turns NMI back on and returns (Mode04Finish).
    LDA #$10
    STA $0100|!addr
    JML $0093F7

; Leaves the game as a screen exit to the entrance leaves it.
enter:
    STZ $0DD5|!addr                 ; no overworld exit
    ; The level's time, from the game's table of them (TimerTable,
    ; $0584D7), as the header load sets it on an entry from the
    ; overworld.
    LDX.b #!time
    LDA.l $0584D7,x
    STA $0F31|!addr
    STZ $0F32|!addr
    STZ $0F33|!addr
    ; A screen exit to the entrance from every screen, so that whichever
    ; the player is on takes it: the entrance's low byte, and in Lunar
    ; Magic's exit format (bit 2) secondary (bit 1) with its bit 8 ...
    LDX #$1F
.exits:
    LDA.b #(!entrance&$FF)
    STA $19B8|!addr,x
    LDA.b #($06|((!entrance>>8)&1))
    STA $19D8|!addr,x
    DEX
    BPL .exits
    ; ... and in the game's own: secondary, with bit 8 from the submap.
    LDA #$01
    STA $1B93|!addr
    LDA.b #((!entrance>>8)&1)
    STA $1F11|!addr
    ; Into a sublevel, as a screen exit goes.
    LDA #$01
    STA $141A|!addr
    RTS
