; Play from here: a ROM that goes from power-on straight into a level, at
; a place the editor chose. "Nintendo Presents" shows for a frame, dark,
; since its game modes set the screen up and decompress the player's
; graphics; then, where the title screen would load (game mode $03), it
; starts a game as the file select does and takes a screen exit to
; secondary entrance !entrance, which the editor adds to the level for it
; (kobo_core::playtest). Where the game would load the overworld (after a
; death, a game over, or the level's end), it enters the level there
; again, with four lives after a game over.
;
; The game uploads the level music bank to the SPC when a level is
; entered from the overworld (UploadLevelMusic, $008134), not into a
; sublevel, as a screen exit is: so the title screen's load uploads it
; here in place of the overworld's (UploadMusicBank1, $00810E), which
; would have the level's song numbers play the overworld's songs.
; AddmusicK, which loads each song itself, leaves that call out of the
; title screen's load, and so does this patch then.
;
; Written from the game's routines as the SMWDisX disassembly documents
; them: GM00LoadPresents ($009391), GM01Presents ($00940F),
; GM03LoadTitleScreen ($0096AE), the new game LoadSaveAndFadeToOW
; ($009E17) starts, GM0CLoadOverworld ($00A087), whose own path into a
; level sets game mode $10 and ends at Mode04Finish ($0093F7), and the
; screen exit path of the entrance code (docs/smw.md).
;
; The overworld sets the translevel the level is entered by ($13BF), which
; the game keeps the midway point by, and patches more (Dragon Coins
; collected, checkpoints): this patch sets it to the level's, or its
; overworld level's for a sublevel.
;
; kkevinm's Retry System respawns the player where it was told to on the
; level's entry from the overworld, which a play build never makes: with
; it (!retry, its retry_config/ram.asm from the project's UberASM Tool
; folder as retry_ram.asm), this patch tells it the entrance, and the
; level's time, as that entry would have told it the level's start. A
; midway point then moves its respawn point as it does in the game.
;
; Defines: !entrance, the secondary entrance (000-1FF), or with
; !secondary 0 the level, entered by its main entrance as a screen exit to
; it enters it (out of a pipe, say, as the level has it); !translevel, the
; translevel; !powerup, the player's power-up (0 small, 1 big, 2 cape, 3
; fire); !switches, the switch palaces pressed (bits 0-3: green, yellow,
; blue, red); !off, 1 to start with the ON/OFF switch off; !time, the
; level's time setting (0-3), which an entry into a sublevel does not load;
; !retry, 1 with the Retry System's RAM.

incsrc "memory.asm"
if !retry
    incsrc "retry_ram.asm"
endif

; The title screen's load checks the overworld override first; the
; overworld's load turns the screen off, clears the level's RAM
; (Clear_1A_13D3: $1A-$D7 and $13D3-$1BA1, the item memory that keeps
; collected coins gone among it), then checks the overworld override, which
; its hook replaces.
assert read1($0096BE) == $AD && read1($0096C1) == $D0, "the title screen's loading code is not the game's"
assert read1($00A093) == $20 && read1($00A096) == $AD && read2($00A097) == $0109, "the overworld's loading code is not the game's"

; "Nintendo Presents": the logo off the screen, and a frame of it.
if read1($0093A4) == $A9 && read1($0093A5) == $70
    org $0093A5
        db $F0
endif
if read1($0093C5) == $A9 && read1($0093C6) == $40
    org $0093C6
        db $01
endif

org $0096BE
    JSL playtest
if read1($0096C3) == $20 && read2($0096C4) == $810E
    JSR $8148                       ; UploadOverworldMusic: the level bank
else
    NOP #3
endif
    JMP $93F7                       ; Mode04Finish: NMI back on, and return

org $00A096
    JML playtest_again

freecode

; Game mode $03, the title screen's load, once memory is cleared.
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
    ; The switch palaces, as a save file keeps them.
    LDX #$03
-   LDA.l .switches,x
    STA $1F27|!addr,x
    DEX
    BPL -
    STZ $0109|!addr                 ; no overworld override
    JSR enter
    ; Into the level, as the overworld's load goes into one at once.
    LDA #$F0
    STA $0DB0|!addr                 ; mosaic, which the fade in takes off
    LDA #$10
    STA $0100|!addr
    RTL

.switches:
    db !switches&1, (!switches>>1)&1, (!switches>>2)&1, (!switches>>3)&1

; Game mode $0C, the overworld's load, once it has turned the screen off
; and cleared the level's RAM, where it checks for a level to go into at
; once: after a death, or the level's end, the level again instead, with
; every coin back.
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
    ; which turns NMI back on and returns (Mode04Finish). Its mosaic too:
    ; the fade in takes $10 off it each step, so from the 0 a fade out
    ; leaves it wraps and ends at $10, the level's layers in 2x2 blocks.
    LDA #$F0
    STA $0DB0|!addr
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
    LDA.b #!translevel
    STA $13BF|!addr
    LDA.b #!off
    STA $14AF|!addr                 ; the ON/OFF switch
if defined("retry_ram_respawn") && defined("retry_ram_timer")
    ; The Retry System's respawn point, in the screen exits' format but
    ; Lunar Magic's bit (2), which it adds, and its copy of the time.
    LDA.b #(!entrance&$FF)
    STA.l !retry_ram_respawn
    LDA.b #((!secondary<<1)|((!entrance>>8)&1))
    STA.l !retry_ram_respawn+1
    LDA $0F31|!addr
    STA.l !retry_ram_timer
    LDA #$00
    STA.l !retry_ram_timer+1
    STA.l !retry_ram_timer+2
endif
    ; A screen exit to the entrance from every screen, so that whichever
    ; the player is on takes it: the entrance's low byte, and in Lunar
    ; Magic's exit format (bit 2) whether secondary (bit 1), with its bit
    ; 8 ...
    LDX #$1F
.exits:
    LDA.b #(!entrance&$FF)
    STA $19B8|!addr,x
    LDA.b #($04|(!secondary<<1)|((!entrance>>8)&1))
    STA $19D8|!addr,x
    DEX
    BPL .exits
    ; ... and in the game's own: whether secondary, with bit 8 from the
    ; submap.
    LDA.b #!secondary
    STA $1B93|!addr
    LDA.b #((!entrance>>8)&1)
    STA $1F11|!addr
    ; Into a sublevel, as a screen exit goes.
    LDA #$01
    STA $141A|!addr
    RTS
