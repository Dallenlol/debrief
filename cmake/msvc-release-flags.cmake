# Included after every project() of the native builds (CMAKE_PROJECT_INCLUDE, set in
# .cargo/config.toml). With the Visual Studio generator the cmake crate replaces
# CMAKE_<LANG>_FLAGS_RELEASE with flags that carry no optimisation level, so whisper.cpp
# (via whisper-rs-sys) compiled with optimisations off: speech recognition ran 10-30x slower
# on every Windows build. Put the standard Release optimisation back. Other compilers and
# builds that already have /O2 are left alone.
if(MSVC)
  foreach(lang C CXX)
    if(NOT CMAKE_${lang}_FLAGS_RELEASE MATCHES "/O2")
      string(APPEND CMAKE_${lang}_FLAGS_RELEASE " /O2 /Ob2 /DNDEBUG")
    endif()
  endforeach()
endif()
