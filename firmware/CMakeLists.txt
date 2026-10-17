# OpenPager firmware. Pick the board profile at configure time:
#   idf.py -B build/onx2432g028 build                                  (default board)
#   idf.py -B build/fnk0104b -DOPENPAGER_BOARD=fnk0104b build
# Each build directory keeps its own sdkconfig.
cmake_minimum_required(VERSION 3.16)

set(OPENPAGER_BOARD "onx2432g028" CACHE STRING "Board profile: onx2432g028 or fnk0104b")
set(SDKCONFIG_DEFAULTS "sdkconfig.defaults;boards/sdkconfig.defaults.${OPENPAGER_BOARD}")
set(SDKCONFIG "${CMAKE_BINARY_DIR}/sdkconfig")

include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(openpager)
