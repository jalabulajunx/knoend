# Board profile: Freenove FNK0104B (Tom). Not written yet: selecting this board fails to link
# until board_fnk0104b.c implements board.h. Use Espressif drivers; Freenove's example code is
# CC BY-NC-SA and must not be copied.
set(srcs "")
if(CONFIG_OPENPAGER_BOARD_FNK0104B)
    # set(srcs "board_fnk0104b.c")
endif()

idf_component_register(SRCS ${srcs} REQUIRES board_api)
