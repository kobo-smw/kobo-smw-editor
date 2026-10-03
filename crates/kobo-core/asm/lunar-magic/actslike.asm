; The acts-like chain and custom block actions, in Lunar Magic's layout.
;
; Kobo's own code, written from Lunar Magic's help ("Map16 Gameplay"),
; GPS's source, the vanilla code it hooks, and the actions a Lunar
; Magic-saved ROM runs, observed with tools/lunar-magic/block-probe
; (docs/lunar-magic-install.md), never from Lunar Magic's code.
;
; The game's four calls to RemapBlocks, for the player, sprites, the cape,
; and fireballs, go to fixed entry points, which send them to Kobo's code.
; It follows the tile through the acts-like tables, then runs the action for
; the kind of contact: an entry block at the address GPS replaces with its
; own, which by default runs the help file's JSL slots. Every action leaves
; through $06F602, which hands the tile to the game's RemapBlocks. Where
; GPS adds contacts of its own, a 4-byte slot at a fixed address is reached
; with A holding the low byte of the return address that tells the contact.
; In the Mode 7 boss battles ($0D9B bit 7) a tile is neither followed nor
; acted on: it goes to the game's RemapBlocks as it is.
;
; Yoshi's berry check, for his tongue and a stunned baby Yoshi, follows the
; chain too, and as Lunar Magic 3.70's does, reads vertical levels and
; layer 2 (below).

incsrc "memory.asm"

; Lunar Magic's gate: its one-time install has been done.
org $06F600
    db $00

; A marker Lunar Magic checks before it reads the acts-like tables through
; $06F624 (docs/lunar-magic-install.md, "Bytes Kobo writes because Lunar
; Magic checks them"): without it, its Map16 editor and export read every
; tile's setting from elsewhere, and a Map16 save would write that back.
org $06F5FC
    db "LM"

; The chain's common exit, which GPS's entries jump to.
org $06F602
    JML chain_exit

; The acts-like tables: pages $00-$3F, and pages $40-$7F (the pointer less
; $8000; bank $FF for none). Two bytes a tile.
org $06F624
    autoclean dl acts_like
org $06F63C
    db $FF

; Entry points, and the slots GPS takes over.
org $06F660
    autoclean JML player_contact
org $06F67B
    JML $06F602
org $06F700
    JML sprite_contact
org $06F717
    JML $06F602
org $06F760
    JML cape_contact
org $06F7A0
    JML fireball_contact

; An action's entry block, as GPS's are: 16 bytes, entered with A, X, and
; Y 8-bit, Y and $1693 the tile reported to the game, $03 the last tile the
; chain looked up. By default it runs the help file's slots for the action.
macro action(entry, slots)
    org <entry>
        JMP <slots>
endmacro
%action($06F690, $F890)       ; the player touched it from below
%action($06F6A0, $F8A0)       ; from above
%action($06F6B0, $F8B0)       ; from the side
%action($06F6C0, $F8C0)       ; its top corner
%action($06F6D0, $F8D0)       ; it is in the player's body
%action($06F6E0, $F8E0)       ; in the player's head
%action($06F720, $F920)       ; a sprite touched it from above or below
%action($06F730, $F930)       ; from the side
%action($06F780, $F980)       ; the cape hit it
%action($06F7C0, $F9C0)       ; a fireball hit it
%action($06F7D0, $F602)       ; wall running, feet (GPS's; no slots)
%action($06F7E0, $F602)       ; wall running, body

; The help file's slots: room for three 4-byte JSLs each, empty until a
; tool or a user writes one over the NOPs.
macro slots(at)
    org <at>
        NOP #12
        JMP $F602
endmacro
%slots($06F890) : %slots($06F8A0) : %slots($06F8B0) : %slots($06F8C0)
%slots($06F8D0) : %slots($06F8E0) : %slots($06F920) : %slots($06F930)
%slots($06F980) : %slots($06F9C0) : %slots($06F9F0)

; The game's calls to RemapBlocks.
org $00F4DD : JSL $06F660    ; the player's interaction points
org $019533 : JSL $06F700    ; a sprite's
org $02961A : JSL $06F760    ; the cape's
org $02A6EB : JSL $06F7A0    ; a fireball's

; Yoshi's berries. The game looks for a berry (tiles $45-$47) where Yoshi's
; tongue is and at a stunned baby Yoshi's centre (CODE_02B9FA), by the tile
; number itself, and on layer 1 of a horizontal level only. Here, as under
; Lunar Magic 3.70 (its "changes", and what its code leaves in memory,
; docs/lunar-magic-install.md, "Yoshi and berries"), the tile is followed
; through the acts-like tables, a vertical level is read as one, and layer
; 2 is looked at too where the level has the player and sprites meet it:
; layer 1 first, and layer 2 only if layer 1 has no berry. A berry on layer
; 2 makes its mushroom where the berry is on layer 1. Yoshi walking into a
; berry (CODE_02D1AD) is the game's, as it is under Lunar Magic.
org $01A24C : JSL berry_check     ; a stunned baby Yoshi
org $01F589 : JSL berry_check     ; Yoshi's tongue
org $02BA9E : JSL berry_tile      ; INC $07 : LDA [$05], the tile's high byte
org $02BAE8 : JSL berry_mushroom  ; JSL InitSpriteTables, for the mushroom

freecode

; The player: A = the tile's high byte, $1693 its low byte, A, X, and Y
; 8-bit. The contact is told by the low byte of the return address the
; interaction point's call to $00F44D pushed, below the JSL's and the
; JSR's in GetBlockAtTouchPos.
player_contact:
    BIT.w $0D9B|!addr
    BPL +                        ; chain_skip is past a branch's reach
    JMP chain_skip
+:
    JSR follow_acts_like
    LDA 6,s
    CMP #$B1 : BEQ .body         ; interaction point 0, centre
    CMP #$26 : BEQ .side         ; 1, side body
    CMP #$3C : BEQ .head         ; 2, side head
    CMP #$8C : BEQ .below        ; 3, head
    CMP #$4C : BEQ .feet         ; 4 and 5, the feet
    CMP #$EB : BEQ .feet
    JML $06F67B
.feet:
    ; A foot within three pixels of the tile's edge is on its corner.
    LDA $9A
    AND #$0F
    CMP #$03 : BCC .corner
    CMP #$0D : BCS .corner
    JML $06F6A0
.corner:
    JML $06F6C0
.below:
    JML $06F690
.side:
    JML $06F6B0
.body:
    JML $06F6D0
.head:
    JML $06F6E0

; A sprite: the return address of the call into the sprite block check
; (CODE_019441) tells a side from above or below. In a vertical level the
; check reaches CODE_019523 through a JSR of its own (at $0194B0), whose
; return address comes first.
sprite_contact:
    BIT.w $0D9B|!addr
    BMI chain_skip
    JSR follow_acts_like
    LDA 4,s
    CMP #$B2 : BNE +             ; the vertical level's JSR CODE_019523
    LDA 6,s
+:
    CMP #$D2 : BEQ .vertical     ; CODE_0192C9
    CMP #$93 : BEQ .horizontal   ; CODE_01928E
    JML $06F717
.vertical:
    JML $06F720
.horizontal:
    JML $06F730

cape_contact:
    BIT.w $0D9B|!addr
    BMI chain_skip
    JSR follow_acts_like
    JML $06F780

fireball_contact:
    BIT.w $0D9B|!addr
    BMI chain_skip
    JSR follow_acts_like
    JML $06F7C0

; A boss battle's tile, A its high byte, straight to the game.
chain_skip:
    JML $00F545

; Every action's end: the game's RemapBlocks, with A the reported high
; byte, returns to the call.
chain_exit:
    TYA
    JML $00F545

; A = the tile's high byte, $1693 its low byte, 8-bit registers. Follows the
; acts-like tables until a tile below $200. Returns Y = its high byte and
; $1693 its low byte, and $03-$04 the last tile looked up. X, P, and $05-$08
; kept: the fireball's code reads its level data pointer at $05 after the
; call.
follow_acts_like:
    PHX
    PHP
    XBA
    LDA $1693|!addr
    REP #$30
    TAX                          ; the tile
    LDA $05
    PHA
    LDA $07
    PHA
    TXA
    ; A bound for a table that loops: a chain of distinct tiles from $200
    ; up is at most $7E00 long, so only a loop runs out.
    LDX #$7E00
.next:
    STA $03
    ASL A
    TAY
    LDA $03
    CMP #$4000
    BCS .upper
    LDA.l $06F624
    STA $05
    LDA.l $06F625
    STA $06
    BRA .read
.upper:
    LDA.l $06F63A
    STA $05
    LDA.l $06F63B
    STA $06
    AND #$FF00
    CMP #$FF00
    BEQ .none
.read:
    LDA [$05],y
    CMP #$0200
    BCC .found
    DEX
    BNE .next
.none:
    LDA #$0130
.found:
    TAY
    PLA
    STA $07
    PLA
    STA $05
    TYA
    SEP #$30
    STA $1693|!addr
    XBA
    TAY
    PLP
    PLX
    RTS

; In place of the game's JSL CODE_02B9FA: $00 and $08 the point's X, $01
; and $09 its Y, on layer 1, X the sprite, A, X, and Y 8-bit. $1933 tells
; the game's GenerateTile which layer the berry was on; it is left 0. X and
; $00-$01 and $08-$09 are kept.
berry_check:
    PHX
    LDA.w $18D6|!addr            ; a berry eaten before, not yet swallowed
    PHA
    LDA #$00
    STA.w $18D6|!addr
    STA.w $1933|!addr
    LDA $5B
    AND #$01                     ; layer 1 is vertical
    JSR berry_layer
    LDA.w $18D6|!addr
    BNE .found
    BIT $5B
    BPL .none                    ; nothing meets layer 2
    ; Layer 2: the point moved by layer 2's position less layer 1's.
    LDA $00 : PHA
    LDA $01 : PHA
    LDA $08 : PHA
    LDA $09 : PHA
    LDA $00 : CLC : ADC $26 : STA $00
    LDA $08 : ADC $27 : STA $08
    LDA $01 : CLC : ADC $28 : STA $01
    LDA $09 : ADC $29 : STA $09
    INC.w $1933|!addr
    LDA $5B
    AND #$02                     ; layer 2 is vertical
    JSR berry_layer
    LDA #$00
    STA.w $1933|!addr
    PLA : STA $09
    PLA : STA $08
    PLA : STA $01
    PLA : STA $00
    LDA.w $18D6|!addr
    BNE .found
.none:
    PLA
    STA.w $18D6|!addr
    PLX
    RTL
.found:
    PLA
    PLX
    RTL

; One layer's berry check, $1933 the layer and Z clear for a vertical one:
; the game's lookup for a horizontal layer (CODE_02BA48), or the one it has
; for a vertical layer and never calls (from $02B9FE), each taking layer
; 2's tables when $0F is 1, and going on to the tile (CODE_02BA92).
berry_layer:
    BEQ .horizontal
    LDA.w $1933|!addr
    STA $0F
    JSL $02B9FE
    RTS
.horizontal:
    LDA.w $1933|!addr
    STA $0F
    JSL $02BA48
    RTS

; The tile's high byte in the berry check, $1693 its low byte: what the
; tile acts like, with Z from the high byte for the game's BNE after it. In
; a boss battle the tile as it is, as for the other contacts.
berry_tile:
    INC $07
    LDA [$05]
    XBA
    LDA.w $0D9B|!addr
    BMI .as_is
    XBA
    JSR follow_acts_like
    TYA
    RTL
.as_is:
    XBA
    RTL

; The mushroom a berry makes, X its slot, placed at the point checked
; ($00, $01, $08, $09): the game's InitSpriteTables, and for a berry on
; layer 2, the position moved back to layer 1's.
berry_mushroom:
    JSL $07F7D2                  ; InitSpriteTables
    LDA.w $1933|!addr
    BEQ .done
    LDA !E4,x : SEC : SBC $26 : STA !E4,x
    LDA !14E0,x : SBC $27 : STA !14E0,x
    LDA !D8,x : SEC : SBC $28 : STA !D8,x
    LDA !14D4,x : SBC $29 : STA !14D4,x
.done:
    RTL

; Until a build writes its own: pages 0 and 1 act as themselves, and the
; rest as tile $130, cement, as a fresh Lunar Magic install has them. Solid
; matters: the game reads high bytes past 1 in some places, the boss
; arenas' floors among them, and treats them as solid.
freedata
acts_like:
    !tile = 0
    while !tile < $4000
        if !tile < $200
            dw !tile
        else
            dw $0130
        endif
        !tile #= !tile+1
    endwhile
