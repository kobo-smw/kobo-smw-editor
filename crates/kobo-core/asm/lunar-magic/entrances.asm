; Secondary entrances past 1FF: the game's four tables and Lunar Magic's two
; further ones (entrance.asm) moved to tables of !entrance_count entrances
; each, behind the pointers Lunar Magic keeps at fixed addresses. The build
; defines it as the last entrance in use plus one, as Lunar Magic 3.70's
; save sizes them: $201 to $2000. A ROM Lunar Magic expanded so has its
; tables in RATS blocks of that size, and its save keeps their pointers and
; reads them as long as their blocks are (docs/lunar-magic.md). Kobo's code
; reads every entrance through the pointers (exits.asm, entrance.asm).
;
; Applied after entrance.asm by a build with an entrance or an exit past
; 1FF, which copies the game's entrances into the moved tables and then
; writes its own.

assert !entrance_count > $200 && !entrance_count <= $2000, "!entrance_count is $201 to $2000"

incsrc "memory.asm"

org $0DE191
    autoclean dl entrance_table_1   ; level, $05F800
org $0DE198
    autoclean dl entrance_table_2   ; $05FA00
org $0DE19F
    autoclean dl entrance_table_3   ; $05FC00
org $05DC81
    autoclean dl entrance_table_4   ; $05FE00
org $05DC86
    autoclean dl entrance_table_5   ; EFYYYYYY
org $05DC8B
    autoclean dl entrance_table_6   ; RLW-----

freedata
entrance_table_1:
    fillbyte $00 : fill !entrance_count
freedata
entrance_table_2:
    fillbyte $00 : fill !entrance_count
freedata
entrance_table_3:
    fillbyte $00 : fill !entrance_count
freedata
entrance_table_4:
    fillbyte $00 : fill !entrance_count
freedata
entrance_table_5:
    fillbyte $00 : fill !entrance_count
freedata
entrance_table_6:
    fillbyte $00 : fill !entrance_count
