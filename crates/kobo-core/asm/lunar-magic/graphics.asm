; Lunar Magic's graphics formats: GFX files 00-33 stored as 4bpp, ExGFX files
; 80-FFF, each level's graphics list, and the older bypass objects 24 and 25.
;
; Kobo's own code, written from the vanilla code it hooks, the formats the
; community documents (smwspeedruns' level data format), Lunar Magic's help,
; and what a Lunar Magic-saved ROM's levels leave in VRAM and RAM
; (docs/lunar-magic-install.md, "Graphics"), never from Lunar Magic's code.
;
; Tables, at the addresses Lunar Magic's layout has them (the build fills
; them): ExGFX 80-FF, 3-byte pointers at $0FF600; the block at read3($0FF873)
; (also at $0FF937): ExGFX 100-FFF, 3-byte pointers, then from read3($0FF7FF)
; one list of 16 words a level (AN2, LT3, BG3, BG2, FG3, BG1, FG2, FG1, SP4,
; SP3, SP2, SP1, LG4, LG3, LG2, LG1), a word's low 12 bits a file and its high
; nibble settings; the older lists, 4 files each (FG3/SP4 first), at $0FF200.
; A pointer of $FFFFFF or $000000 is a file not there.
;
; A level's list is used when bit 15 of its AN2 word is set: every slot is
; loaded, in the order SP1-SP4, FG1, FG2, BG1, FG3, BG2, BG3, each file
; decompressed into the buffer at $7EAD00 and the buffer uploaded whole; a
; slot whose file is 7F or not there keeps the buffer as the slot before left
; it. Then LG1-LG4 to layer 3's tiles with bit 14 set, and the AN2 file into
; the buffer, where the animated tiles read it. Otherwise the object set's and
; sprite set's lists, as the game has them, unless the level's objects 24 and
; 25 name older lists; BG2 and BG3 are then not loaded.
;
; The bytes Lunar Magic checks: $00AAD8 = $EA (4bpp), and $00AA47 = $EA, a
; JSL at $0583B8, and the pointer at $0FF873 (ExGFX), and "LM" at $0FF15C (it
; then keeps the lists as they are); each sits in code Kobo replaces here.

incsrc "memory.asm"

!Buffer = $7EAD00
!List = $0FF7FF
!ExGfx80 = $0FF600
!ExGfx100 = $0FF873
!OldLists = $0FF200

; Direct page scratch, free while graphics load (the game's own loaders use
; $00-$0F).
!Slot = $04                     ; 16-bit
!Count = $06
!Entry = $08                    ; the level's list, 3 bytes
!File = $0B                     ; 16-bit
!Flags = $0D                    ; the AN2 word's high byte

; ---------------------------------------------------------------------------
; The bytes Lunar Magic checks. Each is in code that is dead here: the
; game's UploadSpriteGFX and UploadGFXFile, which jump to Kobo's at their
; starts.
org $00AA47 : db $EA
org $00AAD8 : db $EA
org $0FF15C : db "LM"
; A 4bpp upload's LDX #$10 where the game's 3bpp one has LDX #$07, which
; community patches check before they replace the upload (freeplay's level
; graphics loading optimization). Kobo's upload is elsewhere, so such a
; patch's hook in the dead code here never runs.
org $00AACD : db $A2, $10

; Tables Lunar Magic's editor and code read, none until a build writes them.
org !List : dl $FFFFFF
org !ExGfx100 : dl $FFFFFF
org $0FF937 : dl $FFFFFF
org !ExGfx80
    fillbyte $FF : fill $180
org !OldLists
    fillbyte $00 : fill $400

; ---------------------------------------------------------------------------
; The level load's header read, LDA LevelModeSetting : CMP #$09, which Lunar
; Magic checks for a JSL.
org $0583B8
    autoclean JSL header_level_mode
    NOP

; UploadSpriteGFX, entered with JSR; Kobo's returns through its RTS at $00AA6A.
org $00A9DA
    JML load_graphics

; UploadGFXFile (Y = the file, VRAM's address set), entered with JSR.
org $00AA6B
    JML upload_file

; PrepareGraphicsFile (Y = a file 00-31, decompressed to $7EAD00), entered with
; JSL. Its callers other than UploadGFXFile read the game's 3bpp layout.
org $00BA28
    JML prepare_file
; The game's decompressor (CODE_00B8DE, $8A the source, $00 the destination)
; for code in other banks.
decompress:
    PHP
    SEP #$20
    PHB
    PHK
    PLB
    JSR $B8DE
    PLB
    PLP
    RTL

; GFX33, 4bpp: decompressed where the game expands its 3bpp file to
; ($7E7D00), the expansion skipped, and GFX32 decompressed to $7E2000 as the
; game does.
org $00B895 : db $7D
org $00B89F
    REP #$30
    LDA #$2000                  ; GFX32 where the game has it, $7E2000
    STA $00
    BRA gfx32
org $00B8D7
gfx32:                          ; LDA #GFX32, and on

; The graphics decompression routine Lunar Magic documents: JSL $0FF900 with
; A (16-bit) the file and $00 the 24-bit destination; the processor flags,
; X, and Y kept.
org $0FF900
    JML documented_decompress

; Objects 24 and 25 name the older lists; the loader reads them from the
; level's data, so their routines do nothing (an RTS of the game's).
macro settings(table)
    org <table>+($23*3) : dl $0DAAA3
    org <table>+($24*3) : dl $0DAAA3
endmacro
%settings($0DA455)
%settings($0DC19A)
%settings($0DCD9A)
%settings($0DD99A)
%settings($0DE89A)

freecode

; A 8-bit. Also leaves where the level's list is at $7FC006, for code that
; reads its settings later (the loader below), as a Lunar Magic-saved ROM
; leaves it for every level, boss arenas included, when the ROM has lists.
header_level_mode:
    PHP
    REP #$30
    LDA.l !List+1
    CMP #$FFFF
    BEQ +
    LDA.l $00010B|!addr
    AND #$01FF
    ASL A
    ASL A
    ASL A
    ASL A
    ASL A
    CLC
    ADC.l !List
    STA.l $7FC006
    SEP #$20
    LDA.l !List+2
    ADC #$00
    STA.l $7FC008
+   PLP
    LDA.l $001925|!addr
    CMP #$09
    RTL

; ---------------------------------------------------------------------------
; A (16-bit) = a file 00-31 or 80-FFF. Sets $8A-$8C to where it is stored, and
; returns with carry clear, or carry set when it is not there. X, Y kept.
find_file:
    PHX
    CMP #$0032
    BCS .ex
    TAX
    SEP #$20
    LDA.l $00B992,x
    STA $8A
    LDA.l $00B9C4,x
    STA $8B
    LDA.l $00B9F6,x
    STA $8C
    REP #$20
    BRA .check
.ex:
    CMP #$0080
    BCC .none
    CMP #$0100
    BCS .high
    SEC
    SBC #$0080
    STA $8A
    ASL A
    ADC $8A                     ; carry clear
    TAX
    LDA.l !ExGfx80,x
    STA $8A
    LDA.l !ExGfx80+1,x
    STA $8B
    BRA .check
.high:
    CMP #$1000
    BCS .none
    SEC
    SBC #$0100
    STA $8A
    ASL A
    ADC $8A
    CLC
    ADC.l !ExGfx100
    STA $8A
    SEP #$20
    LDA.l !ExGfx100+2
    ADC #$00
    STA $8C
    REP #$20
    LDA.l !ExGfx100+1           ; the block itself may be missing
    CMP #$FFFF
    BEQ .none
    LDA [$8A]                   ; the file's pointer, into $8D-$8F
    STA $8D
    INC $8A
    LDA [$8A]
    STA $8E
    LDA $8D
    STA $8A
    SEP #$20
    LDA $8F
    STA $8C
    REP #$20
.check:
    LDA $8B
    CMP #$FFFF
    BEQ .none
    ORA $8A
    BEQ .none
    PLX
    CLC
    RTS
.none:
    PLX
    SEC
    RTS

; A (16-bit) = the file. Decompresses it into the buffer. Carry set if it
; is not there, and the buffer is as it was. X and Y kept.
decompress_file:
    PHX
    PHY
    JSR find_file
    BCS .done
    LDA.w #!Buffer
    STA $00
    SEP #$20
    LDA.b #!Buffer>>16
    STA $02
    REP #$20
    JSL decompress
    CLC
.done:
    PLY
    PLX
    RTS

; X (16-bit) = the VRAM word address, A = bytes. Uploads the buffer's first
; A bytes there through DMA (!dma).
upload_buffer:
    LDY #$0000
; The same from Y bytes into the buffer.
upload_from:
    STX $2116
    PHA
    SEP #$20
    LDA #$80
    STA $2115
    REP #$20
    PLA
    BRA +
; The same where VRAM's address register already points.
dma_buffer:
    LDY #$0000
+   STA !dma+5
    SEP #$20
    LDA #$01
    STA !dma
    LDA #$18
    STA !dma+1
    LDA.b #!Buffer>>16
    STA !dma+4
    REP #$20
    TYA
    CLC
    ADC.w #!Buffer
    STA !dma+2
    SEP #$20
    LDA #!dma_bit
    STA $420B
    REP #$20
    RTS

; ---------------------------------------------------------------------------
; UploadGFXFile: Y (8-bit) the file, VRAM's address set. The file is 4bpp,
; uploaded as it is. As the game does, file 01 is replaced by file 31 once
; the special world is passed.
upload_file:
    PHP
    PHB
    PHK
    PLB
    REP #$30
    TYA
    AND #$00FF
    CMP #$0001
    BNE +
    LDA.l $001EEB|!addr         ; OWLevelTileSettings+$49
    AND #$0080
    BEQ ++
    LDA #$0031
    BRA +
++  LDA #$0001
+   JSR decompress_file
    LDA #$1000
    JSR dma_buffer
    PLB
    PLP
    JML $00AA6A                 ; RTS

; ---------------------------------------------------------------------------
; PrepareGraphicsFile: Y (8-bit) = a file 00-31. Decompresses it into the
; buffer and, for a file the game keeps as 3bpp, puts it in the game's 3bpp
; layout there, for the callers that read that (the MARIO START and GAME OVER
; letters, sprite tiles 4A-4F and 5A-5F, the overworld's animated tiles, the
; Mode 7 bosses' tiles). Returns with $00-$02 = $7EAD00 and Y kept, as the
; game's does.
prepare_file:
    PHP
    REP #$30
    PHB
    PHY
    PHK
    PLB
    TYA
    AND #$00FF
    PHA
    JSR decompress_file
    PLA
    CMP #$0027
    BCC .3bpp
    CMP #$002C
    BCC .done
    CMP #$002F
    BCC .3bpp
    BEQ .done
    CMP #$0032
    BCS .done
.3bpp:
    ; 128 tiles of 32 bytes to 24: planes 0 and 1 as they are, then plane 2,
    ; the even bytes of the tile's second half.
    LDX #$0000                  ; from
    LDY #$0000                  ; to
.tile:
    PHX
    LDA #$0008
    STA $0E
-   LDA.l !Buffer,x
    PHX
    TYX
    STA.l !Buffer,x
    PLX
    INX
    INX
    INY
    INY
    DEC $0E
    BNE -
    LDA #$0008
    STA $0E
    SEP #$20
-   LDA.l !Buffer,x
    PHX
    TYX
    STA.l !Buffer,x
    PLX
    INX
    INX
    INY
    DEC $0E
    BNE -
    REP #$20
    PLX
    TXA
    CLC
    ADC #$0020
    TAX
    CPX #$1000
    BCC .tile
.done:
    LDA.w #!Buffer
    STA $00
    SEP #$20
    LDA.b #!Buffer>>16
    STA $02
    PLY
    PLB
    PLP
    RTL

; A (16-bit) = the file, $00 = the destination. Kept: P, X, Y.
documented_decompress:
    PHP
    REP #$30
    PHX
    PHY
    JSR find_file
    BCS +
    JSL decompress
+   PLY
    PLX
    PLP
    RTL

; ---------------------------------------------------------------------------
; UploadSpriteGFX: A, X, Y 8-bit. Returns through the game's RTS.
;
; In a level's load (game mode $12) with lists in the ROM, the level's list
; decides: with bit 15 of its AN2 word (G), its slots and AN2; with bit 13
; (T), layer 3's tilemap from LT3; with bit 14 (3), layer 3's files from
; LG1-LG4, and without it, once any level's list has bit 14, the game's
; own layer 3 files (28-2B) again. Without G, the sets' lists, or the older
; lists objects 24 and 25 name. The order: sprites and FG/BG, the tilemap,
; layer 3's files, AN2 last, as the buffer is left for the animated tiles.
load_graphics:
    PHP
    PHB
    PHK
    PLB
    REP #$30
    ; The direct page this code borrows, given back at the end (the game's
    ; code reads what a load leaves there: the player's tile pointers,
    ; before the player is set up, are built from $0C-$0D).
    LDX #$000A
-   LDA $04,x
    PHA
    DEX
    DEX
    BPL -
    STZ !Flags
    LDA $0100|!addr
    AND #$00FF
    CMP #$0012
    BNE .standard
    LDA.l !List+1
    CMP #$FFFF
    BEQ .standard
    LDA.l $7FC006               ; the level's list (header_level_mode)
    STA !Entry
    LDA.l $7FC007
    STA !Entry+1
    LDA [!Entry]
    XBA
    AND #$00F0
    ORA #$0001                  ; bit 0: the list is there
    STA !Flags
    ; $7FC01A bit 7 set with T, clear without, as a Lunar Magic-saved
    ; ROM's load leaves it (layer3.asm keeps the low nibble there).
    SEP #$20
    AND #$20
    ASL A
    ASL A
    STA.l $7FC01A
    REP #$20
    LDA !Flags
    AND #$0080
    BEQ .standard
    JSR load_slots
    JSR tileset_files
    BRA .layer3
.standard:
    JSR load_standard
    LDA !Flags                  ; the list's place again, for layer 3's
    AND #$0001
    BEQ +
    LDA.l $7FC006
    STA !Entry
    LDA.l $7FC007
    STA !Entry+1
+   ; A Mode 7 object set ($FE, $FF): the game's own code from there on.
    LDA $1931|!addr
    AND #$00FF
    CMP #$00FE
    BCC .layer3
    JSR load_layer3
    JSR load_animated
    JSR give_back
    SEP #$20
    LDA #$FF                    ; as the game's loader leaves it here
    STA $0E
    PLB
    PLP
    SEP #$30
    LDA $1931|!addr
    CMP #$FE
    JML $00AA5B                 ; SetallFGBG80
.layer3:
    JSR load_layer3
    JSR load_animated
    JSR give_back
    PLB
    PLP
    JML $00AA6A                 ; RTS

; The direct page load_graphics borrowed, as it was, and $0F at $FF as the
; game's own loader leaves it. A, X, Y 16-bit; called with the six words on
; the stack under its return address.
give_back:
    PLY                         ; the return address
    LDX #$0000
-   PLA
    STA $04,x
    INX
    INX
    CPX #$000C
    BCC -
    SEP #$20
    LDA #$FF
    STA $0F
    REP #$20
    PHY
    RTS

; The list's slots, as (word in the list * 2, VRAM word address), in the
; order they load.
list_slots:
    dw 11*2, $6000, 10*2, $6800, 9*2, $7000, 8*2, $7800
    dw 7*2, $0000, 6*2, $0800, 5*2, $1000, 4*2, $1800
    dw 3*2, $2000, 2*2, $2800
list_layer3:
    dw 15*2, $4000, 14*2, $4400, 13*2, $4800, 12*2, $4C00

tilemap_sizes:
    dw $2000, $1000, $0800, $2000, $2000, $1000, $0800, $2000
    dw $2000, $1000, $0800, $2000, $2000, $1000, $0800, $2000
tilemap_places:
    dw $50A0, $5000, $5080, $5800
tilemap_skips:
    dw $0140, $0000, $0100, $0000

; A, X, Y 16-bit.
load_slots:
    LDX #$0000
-   LDA.w list_slots+2,x
    PHA
    LDA.w list_slots,x
    TAY
    LDA [!Entry],y
    AND #$0FFF
    CMP #$007F
    BEQ +
    JSR decompress_file
+   PLA
    PHX
    TAX
    LDA #$1000
    JSR upload_buffer
    PLX
    INX
    INX
    INX
    INX
    CPX #$0028
    BCC -
    RTS

; Layer 3's tilemap (T) and files (3, or the game's own again). A, X, Y
; 16-bit.
load_layer3:
    LDA !Flags
    AND #$0020
    BNE +
    JMP .files
+
    ; LT3's file; its high nibble DDFF the size (FF: 0 $2000 bytes, 1
    ; $1000, 2 $800; builds refuse 3, which Lunar Magic loads otherwise) and
    ; where it goes (DD: 0 under the status bar, $50A0, the file's first
    ; $140 bytes left out; 1 the tilemap's start, $5000, all of it; 2 from
    ; the status bar's last row, $5080, $100 left out; 3 the bottom half,
    ; $5800, all of it).
    LDY #$0002
    LDA [!Entry],y
    STA !File
    XBA
    LSR A
    LSR A
    LSR A
    LSR A
    AND #$000F
    STA !Slot                   ; DDFF
    ASL A
    TAX
    LDA.w tilemap_sizes,x
    CMP #$2000
    BNE +
    JSR park                    ; a $2000-byte file runs past the buffer
+   LDA !File
    AND #$0FFF
    CMP #$007F
    BEQ +
    JSR decompress_file
+   LDA !Slot
    ASL A
    TAX
    LDA.w tilemap_sizes,x
    PHA
    LDA !Slot
    LSR A
    AND #$0006
    TAX
    LDA.w tilemap_skips,x
    TAY
    STA $00
    PLA
    SEC
    SBC $00
    PHA
    LDA.w tilemap_places,x
    TAX
    PLA
    JSR upload_from
    LDA !Slot
    ASL A
    TAX
    LDA.w tilemap_sizes,x
    CMP #$2000
    BNE .files
    JSR unpark
    ; What was put aside stays in VRAM at $4000-$47FF, as a Lunar Magic-saved
    ; ROM's load leaves it, until layer 3's files go over it below (with 3, or
    ; the game's own when another level's list has 3).
.files:
    LDA !Flags
    AND #$0040
    BNE .list
    LDA !Flags
    AND #$0001
    BEQ .done
    JSR any_layer3              ; carry set: some level has bit 14
    BCC .done
    ; The game's own files, 28-2B.
    LDX #$0000
-   TXA
    LSR A
    LSR A
    CLC
    ADC #$0028
    JSR decompress_file
    LDA.w list_layer3+2,x
    PHX
    TAX
    LDA #$0800
    JSR upload_buffer
    PLX
    INX
    INX
    INX
    INX
    CPX #$0010
    BCC -
.done:
    RTS
.list:
    LDX #$0000
-   LDA.w list_layer3+2,x
    PHA
    LDA.w list_layer3,x
    TAY
    LDA [!Entry],y
    AND #$0FFF
    CMP #$007F
    BEQ +
    JSR decompress_file
+   PLA
    PHX
    TAX
    LDA #$0800
    JSR upload_buffer
    PLX
    INX
    INX
    INX
    INX
    CPX #$0010
    BCC -
    RTS

; Carry set if any level's list has bit 14 (layer 3's files) set. X kept.
any_layer3:
    PHX
    LDA.l !List
    STA $00
    SEP #$20
    LDA.l !List+2
    STA $02
    REP #$20
    LDX #$0200
-   LDA [$00]
    AND #$4000
    BNE .yes
    LDA $00
    CLC
    ADC #$0020
    STA $00
    BCC +
    SEP #$20
    INC $02
    REP #$20
+   DEX
    BNE -
    PLX
    CLC
    RTS
.yes:
    PLX
    SEC
    RTS

; AN2: the list's (with G) or object 25's, into the buffer, last. A, X, Y
; 16-bit.
load_animated:
    LDA !Flags
    AND #$0080
    BEQ .old
    LDA [!Entry]
    AND #$0FFF
    CMP #$007F
    BEQ .done
    JMP decompress_file
.old:
    LDA !Count                  ; object 25's file + 1 (load_standard)
    AND #$00FF
    BEQ .done
    DEC A
    JMP decompress_file
.done:
    RTS

; A $2000-byte layer 3 tilemap decompressed into the buffer runs over
; $7EBD00-$7ECCFF, the $1000 bytes after it. park puts them aside in VRAM at
; word address $4000-$47FF, where layer 3's files go later, and unpark brings
; them back, leaving the copy there (docs/lunar-magic-install.md, "Layer 3 in
; the lists"). The screen is off. A, X, Y 16-bit; the flags kept, A changed.
park:
    PHP
    SEP #$20
    LDA #$80                    ; step after the high byte
    STA $2115
    REP #$20
    LDA #$4000
    STA $2116
    LDA #$1801                  ; A to B, two registers alternating: $2118
    STA !dma
    BRA park_transfer

; The read port reads ahead: setting the address fills its latch with the
; word there, and a read that steps the address refills the latch from the
; address before stepping it, so the first word read after setting the
; address comes again on the next. One word is read and dropped before the
; transfer, which then starts at $4000.
unpark:
    PHP
    SEP #$20
    LDA #$80                    ; step after the high byte, $213A
    STA $2115
    REP #$20
    LDA #$4000
    STA $2116
    LDA $2139                   ; $2139 then $213A: the word dropped
    LDA #$3981                  ; B to A, two registers alternating: $2139
    STA !dma

; The DMA itself, from or to $7EBD00, $1000 bytes; PHP done by the caller.
park_transfer:
    LDA.w #!Buffer+$1000
    STA !dma+2
    LDA #$1000
    STA !dma+5
    SEP #$20
    LDA.b #!Buffer>>16
    STA !dma+4
    LDA #!dma_bit
    STA $420B
    PLP
    RTS

; The files the game believes are loaded ($0101-$0108): the sprite set's and
; object set's, as its own loader leaves them, whatever a list loaded.
tileset_files:
    SEP #$30
    LDA $192B|!addr
    ASL A
    ASL A
    TAX
    LDY #$03
-   LDA.l $00A8C3,x             ; SPRITEGFXLIST
    STA $0101|!addr,y
    INX
    DEY
    BPL -
    LDA $1931|!addr
    CMP #$FE
    BCS +
    ASL A
    ASL A
    TAX
    LDY #$03
-   LDA.l $00A92B,x             ; OBJECTGFXLIST
    STA $0105|!addr,y
    INX
    DEY
    BPL -
+   REP #$30
    RTS

; The game's loading by the sets' lists, or by the older lists objects 24 and
; 25 name: a file already where it goes is not loaded again. A, X, Y 16-bit.
load_standard:
    JSR find_old_objects        ; !Slot: SP list+1, !Slot+1: FG list+1, !Count: AN2+1
    SEP #$30
    ; Sprites: the four files into $04-$07, SP1 in $07 as the game has it.
    LDA $192B|!addr
    ASL A
    ASL A
    TAX
    LDY #$03
-   LDA.l $00A8C3,x
    STA $0008|!dp,y
    INX
    DEY
    BPL -
    LDA !Slot
    BEQ .sprites
    DEC A
    JSR old_list                ; $08-$0B from the older list, loaded whole
    LDX #$03
-   LDA $08,x
    JSR standard_slot
    DEX
    BPL -
    BRA .objects
.sprites:
    LDX #$03
-   LDA $08,x
    CMP $0101|!addr,x
    BEQ +
    STA $0101|!addr,x
    JSR standard_slot
+   DEX
    BPL -
.objects:
    LDA $1931|!addr
    CMP #$FE
    BCS .done
    ASL A
    ASL A
    TAX
    LDY #$03
-   LDA.l $00A92B,x
    STA $0008|!dp,y
    INX
    DEY
    BPL -
    LDA !Slot+1
    BEQ .fg
    DEC A
    JSR old_list
    LDX #$07
-   LDA $08-4,x
    JSR standard_slot
    DEX
    CPX #$04
    BCS -
    BRA .an2
.fg:
    LDX #$03
-   LDA $08,x
    CMP $0105|!addr,x
    BEQ +
    STA $0105|!addr,x
    TXA
    ORA #$04
    TAX
    LDA $08-4,x
    JSR standard_slot
    TXA
    AND #$03
    TAX
+   DEX
    BPL -
.an2:
    LDA !Slot
    ORA !Slot+1
    BEQ .done
    REP #$30
    JSR tileset_files           ; the files the game believes loaded
.done:
    REP #$30
    RTS

; A (8-bit) = an older list's index: its four files into $08-$0B.
old_list:
    REP #$30
    AND #$00FF
    ASL A
    ASL A
    TAX
    LDY #$0000
-   LDA.l !OldLists,x
    SEP #$20
    STA $0008|!dp,y
    REP #$20
    INX
    INY
    CPY #$0004
    BCC -
    SEP #$30
    RTS

; A (8-bit) = the file, X = the slot (0-3 sprites SP4 to SP1, 4-7 FG3 to
; FG1): decompressed (file 01 as the game does) and uploaded. X kept.
standard_slot:
    PHX
    REP #$30
    AND #$00FF
    CMP #$0001
    BNE +
    LDA.l $001EEB|!addr
    AND #$0080
    BEQ ++
    LDA #$0031
    BRA +
++  LDA #$0001
+   JSR decompress_file
    PLX
    PHX
    TXA
    AND #$00FF
    ASL A
    TAX
    LDA.w standard_vram,x
    TAX
    LDA #$1000
    JSR upload_buffer
    SEP #$30
    PLX
    RTS

standard_vram:
    dw $7800, $7000, $6800, $6000, $1800, $1000, $0800, $0000

; Objects 24 and 25 in the level's layer 1 data: !Slot = SP list + 1,
; !Slot+1 = FG list + 1, !Count = AN2 + 1, each 0 if none. A, X, Y 16-bit.
find_old_objects:
    STZ !Slot
    STZ !Count
    LDA $010B|!addr
    AND #$01FF
    STA !Entry
    ASL A
    ADC !Entry
    TAX
    LDA.l $05E000,x
    STA !Entry
    LDA.l $05E001,x
    STA !Entry+1
    LDY #$0005                  ; past the header
.object:
    SEP #$20
    LDA [!Entry],y
    CMP #$FF
    BNE +
    REP #$20
    RTS
+   STA $02                     ; the first byte
    INY
    LDA [!Entry],y
    STA $03                     ; the second
    INY
    LDA $02
    AND #$60
    LSR A
    STA $00
    LDA $03
    LSR A
    LSR A
    LSR A
    LSR A
    ORA $00                     ; the object's number
    BNE .standard
    LDA [!Entry],y              ; an extended object
    INY
    CMP #$00
    BNE +
    INY                         ; a screen exit, four bytes
    BRA .object
+   CMP #$02
    BNE .object
    INY                         ; a long screen exit, five
    INY
    BRA .object
.standard:
    CMP #$22
    BEQ .four
    CMP #$23
    BEQ .four
    CMP #$2D
    BEQ .five
    CMP #$27
    BEQ .map16
    CMP #$29
    BEQ .map16
    CMP #$24
    BNE +
    LDA $02                     ; SSSS
    ASL A
    ASL A
    ASL A
    ASL A
    STA !Slot
    LDA $03
    AND #$0F
    ORA !Slot
    STA !Slot
    LDA [!Entry],y
    STA !Slot+1
    BRA .three
+   CMP #$25
    BNE .three
    LDA [!Entry],y
    STA !Count
.three:
    INY
    JMP .object
.four:
    INY
    INY
    JMP .object
.five:
    INY
    INY
    INY
    JMP .object
.map16:
    ; Five to eight bytes by the fourth byte's top bits, and the third's.
    INY
    LDA [!Entry],y              ; the fourth
    DEY
    AND #$C0
    CMP #$80
    BCC .five
    BEQ .six
    LDA [!Entry],y              ; the third
    BMI .eight
    INY                         ; seven
    BRA .six
.eight:
    INY
    INY
.six:
    INY
    INY
    INY
    INY
    JMP .object
