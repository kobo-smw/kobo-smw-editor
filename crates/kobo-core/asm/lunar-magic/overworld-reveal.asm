; Lunar Magic's option to turn the overworld's event path fade off, with
; a path reveal speed: each step of an event's layer 2 path comes after
; its timer (ColorFadeTimer) has gone up by !speed a frame to $40, with no
; fading tiles drawn as sprites or colours changed for them
; (docs/lunar-magic-install.md, "The overworld").
;
; Kobo's own code, written from the game's (CODE_04EAA4 takes a step once
; the timer reaches $40; CODE_04EAC9 fades the tile in meanwhile) and the
; frames a step takes in Lunar Magic-saved ROMs: ceil($40 / speed) + 1.
; Applied after overworld.asm, with !speed defined (1 to $40).

incsrc "memory.asm"

!fade_timer = $1495|!addr

; In place of the fade's frame (CODE_04EAC9), which the step's caller
; enters while the timer is under $40.
org $04EAC9
    JSL reveal_frame
    RTS

freecode
reveal_frame:
    LDA !fade_timer
    CLC
    ADC.b #!speed
    STA !fade_timer
    RTL
