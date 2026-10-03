; A logging routine in the help file's Yoshi's tongue slot ($06F9F0): it
; counts its calls at $7FB40F and keeps Y, $1693, and $03-$04 of the last.
lorom
org $06F9F0
    JSL probe
freecode
probe:
    PHP
    SEP #$20
    LDA $7FB40F
    INC A
    STA $7FB40F
    TYA
    STA $7FB410
    LDA $1693
    STA $7FB411
    LDA $03
    STA $7FB412
    LDA $04
    STA $7FB413
    PLP
    RTL
