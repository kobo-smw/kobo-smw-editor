; Logs each action: its number, then the game's touch position ($98-$9B),
; Mario's position ($94-$97), then Y, $1693, $03-$04, and the player's $77, $72,
; and $92, at $7FB410 + 16 * n,
; n counted at $7FB40F.
; $7FB40F-$7FB4FF is assumed free: SMWDisX's RAM map names nothing there, and
; GPS 1.4.4 does not use it.
db $37
JMP MarioBelow : JMP MarioAbove : JMP MarioSide
JMP SpriteV : JMP SpriteH : JMP MarioCape : JMP MarioFireball
JMP TopCorner : JMP BodyInside : JMP HeadInside
JMP WallFeet : JMP WallBody

MarioBelow: LDA #$00 : JMP Log
MarioAbove: LDA #$01 : JMP Log
MarioSide: LDA #$02 : JMP Log
SpriteV: LDA #$03 : JMP Log
SpriteH: LDA #$04 : JMP Log
MarioCape: LDA #$05 : JMP Log
MarioFireball: LDA #$06 : JMP Log
TopCorner: LDA #$07 : JMP Log
BodyInside: LDA #$08 : JMP Log
HeadInside: LDA #$09 : JMP Log
WallFeet: LDA #$0A : JMP Log
WallBody: LDA #$0B : JMP Log
Log:
  PHX : PHA
  LDA $7FB40F : CMP #$0F : BCS .full
  ASL : ASL : ASL : ASL : TAX
  PLA : STA $7FB410,x : PHA
  LDA $98 : STA $7FB411,x
  LDA $99 : STA $7FB412,x
  LDA $9A : STA $7FB413,x
  LDA $9B : STA $7FB414,x
  TYA : STA $7FB419,x
  LDA $1693 : STA $7FB41A,x
  LDA $03 : STA $7FB41B,x
  LDA $04 : STA $7FB41C,x
  LDA $77 : STA $7FB41D,x
  LDA $72 : STA $7FB41E,x
  LDA $92 : STA $7FB41F,x
  LDA $94 : STA $7FB415,x
  LDA $95 : STA $7FB416,x
  LDA $96 : STA $7FB417,x
  LDA $97 : STA $7FB418,x
  LDA $7FB40F : INC : STA $7FB40F
.full:
  PLA : PLX
  RTL
print "probe"
