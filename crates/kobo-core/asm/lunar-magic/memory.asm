; Where the ROM maps and where the game keeps its variables, for every patch
; in this folder, which includes this file first. A vanilla ROM is LoROM
; with the game's variables in work RAM; an SA-1 ROM is SA-1 Pack's
; (docs/sa1.md), which moves them where both of its processors reach them.
; ram::RamMap is the same map for the library, and tests/install.rs checks
; that the two agree.
;
; How a patch names a variable, by its vanilla address:
;   $00-$FF        direct page, as the game addresses it ($0E); written as
;                  an absolute address, $000E|!dp
;   $0100-$1FFF    absolute, $010B|!addr; long, $00010B|!addr. A define
;                  puts the address last, !addr|$0BF6, so that !name+2
;                  means the same whichever order Asar evaluates | and +
;                  in; an operand written out has nothing added after it.
;   $7EC800        the tile grid's low bytes, !map16_low; high, !map16_high
;   $1938          the sprite load flags, !sprite_load_flags (!load_flags
;                  of them)
;   $9E, $14D4...  a sprite table, by its vanilla address: !9E,x, !14D4,x;
;                  SA-1 Pack packs them elsewhere, with 22 slots
;                  (more_sprites/sprite_tables.asm)
; A DMA from a buffer in $0100-$1FFF reads it from bank $00 at the address
; with !addr, as SA-1 Pack's own uploads do. Uploads in the NMI or with the
; screen off go through channel !dma ($43x0, its bit in $420B !dma_bit):
; the game's 1, which SA-1 Pack moves to 2, keeping 1 for HDMA
; (remap/dma.asm).
; Work RAM past $1FFF ($7E2000 on, $7F0000 on) stays where it is, and only
; the S-CPU reaches it, or the PPU's registers: under SA-1 Pack, code the
; SA-1 runs must leave those to the S-CPU (!sa1, docs/sa1.md).

if read1($00FFD5) == $23
    if read1($00FFD7) >= $0D
        fullsa1rom
    else
        sa1rom
    endif
    !sa1 = 1
    !dp = $3000
    !addr = $6000
    !map16_low = $40C800
    !map16_high = $41C800
    !sprite_load_flags = $418A00
    !load_flags = $FF
    !dma = $4320
    !dma_bit = $04
    !9E = $3200
    !D8 = $3216
    !E4 = $322C
    !14D4 = $3258
    !14E0 = $326E
    !167A = $7616
    !187B = $3410
else
    lorom
    !sa1 = 0
    !dp = $0000
    !addr = $0000
    !map16_low = $7EC800
    !map16_high = $7FC800
    !sprite_load_flags = $001938
    !load_flags = $80
    !dma = $4310
    !dma_bit = $02
    !9E = $9E
    !D8 = $D8
    !E4 = $E4
    !14D4 = $14D4
    !14E0 = $14E0
    !167A = $167A
    !187B = $187B
endif

; Has the S-CPU run <routine> for code that may run on the SA-1, which
; cannot reach work RAM past $1FFF or the PPU: SA-1 Pack's call from the
; SA-1 (engine/snes_irq.asm), which takes the routine's address in
; $0183-$0185 and $D0 in $2209, and sets $018A once the routine has
; returned. The routine runs with A, X, and Y 8-bit and the direct page at
; $3000, and ends with RTL; values go both ways through the direct page,
; which the two processors share. On the S-CPU, and on a LoROM ROM, the
; routine is called directly. A, X, and Y 8-bit; A is changed.
macro call_scpu(routine)
    if !sa1
        TSC
        XBA
        CMP #$37                ; the SA-1's stack page
        BNE ?direct
        LDA.b #<routine>
        STA.l $000183
        LDA.b #<routine>>>8
        STA.l $000184
        LDA.b #<routine>>>16
        STA.l $000185
        LDA #$D0
        STA.l $002209
    ?wait:
        LDA.l $00018A
        BEQ ?wait
        LDA #$00
        STA.l $00018A
        BRA ?done
    ?direct:
        JSL <routine>
    ?done:
    else
        JSL <routine>
    endif
endmacro
