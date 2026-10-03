; Choc Island 2's rooms with levels 0CD-0CF moved. Entered through a screen
; exit of translevel $24, those levels load one of the game's own rooms
; instead, chosen by coins and time (CODE_05DAEF): CODE_05DB49 takes the
; room's layer 1, sprite, and layer 2 pointers from 16-bit tables, and the
; banks stay those of the level the exit led to. They are the rooms' banks
; ($06, $07, $0C) only while that level's data is where the game has it; a
; build that writes one of the three elsewhere gives the rooms their banks
; here, as a Lunar Magic-saved ROM leaves them (docs/lunar-magic-install.md,
; "Choc Island 2's rooms").
;
; Kobo's own code, written from the vanilla code it hooks (SMWDisX) and what
; the game and a Lunar Magic-saved ROM leave in RAM once a room has loaded,
; never from Lunar Magic's code.

incsrc "memory.asm"

; The room's layer 1 pointer, A 16-bit, X the room times two (8-bit):
; LDA.L ChocIsld2Layer1,X ($05DB08), then STA $65.
org $05DB4B
    autoclean JSL banks

freecode

banks:
    SEP #$20
    LDA #$06                    ; the rooms' layer 1 data
    STA $67
    LDA #$07                    ; their sprites
    STA $D0
    LDA #$0C                    ; their layer 2 (DATA_0CDF59)
    STA $6A
    REP #$20
    LDA.l $05DB08,x
    RTL
