# CMake script to build DPDK deb package
# Usage: cmake -P dpdk_deb.cmake <deb_path> <pkg_dir> <dpdk_build_dir>
#
# Skips building if the deb file already exists

cmake_minimum_required(VERSION 3.16)

# Get arguments
set(DEB_PATH "${CMAKE_ARGV3}")
set(PKG_DIR "${CMAKE_ARGV4}")
set(DPDK_BUILD_DIR "${CMAKE_ARGV5}")

# Check if deb already exists
if(EXISTS "${DEB_PATH}")
  message(STATUS "DPDK .deb package already exists: ${DEB_PATH}")
  message(STATUS "Skipping build. Delete the file to rebuild.")
  return()
endif()

message(STATUS "Building DPDK .deb package...")

# Clean and create package directory
file(REMOVE_RECURSE "${PKG_DIR}")
file(MAKE_DIRECTORY "${PKG_DIR}/DEBIAN")

# Run ninja install with DESTDIR
execute_process(
  COMMAND ${CMAKE_COMMAND} -E env DESTDIR=${PKG_DIR} ninja -C ${DPDK_BUILD_DIR} install
  RESULT_VARIABLE result
)
if(NOT result EQUAL 0)
  message(FATAL_ERROR "Failed to run ninja install")
endif()

# Remove example source code (always installed by DPDK regardless of -Dexamples option)
file(REMOVE_RECURSE "${PKG_DIR}/opt/dpdk/share/dpdk/examples")

# Copy DEBIAN package files from cmake directory
# Note: dev packages needed for pkg-config dependencies (Requires.private in libdpdk.pc)
file(COPY "${CMAKE_CURRENT_LIST_DIR}/pkg/control"
  DESTINATION "${PKG_DIR}/DEBIAN"
  FILE_PERMISSIONS OWNER_READ OWNER_WRITE GROUP_READ WORLD_READ
)
file(COPY "${CMAKE_CURRENT_LIST_DIR}/pkg/postinst" "${CMAKE_CURRENT_LIST_DIR}/pkg/postrm"
  DESTINATION "${PKG_DIR}/DEBIAN"
  FILE_PERMISSIONS OWNER_READ OWNER_WRITE OWNER_EXECUTE GROUP_READ GROUP_EXECUTE WORLD_READ WORLD_EXECUTE
)

# Install profile.d script to set up PKG_CONFIG_PATH
file(MAKE_DIRECTORY "${PKG_DIR}/etc/profile.d")
file(COPY "${CMAKE_CURRENT_LIST_DIR}/pkg/dpdk-net.sh"
  DESTINATION "${PKG_DIR}/etc/profile.d"
  FILE_PERMISSIONS OWNER_READ OWNER_WRITE GROUP_READ WORLD_READ
)

# Install ld.so.conf.d config for library path
file(MAKE_DIRECTORY "${PKG_DIR}/etc/ld.so.conf.d")
file(COPY "${CMAKE_CURRENT_LIST_DIR}/pkg/dpdk-net.conf"
  DESTINATION "${PKG_DIR}/etc/ld.so.conf.d"
  FILE_PERMISSIONS OWNER_READ OWNER_WRITE GROUP_READ WORLD_READ
)

# Build deb package
execute_process(
  COMMAND dpkg-deb --build "${PKG_DIR}" "${DEB_PATH}"
  RESULT_VARIABLE result
)
if(NOT result EQUAL 0)
  message(FATAL_ERROR "Failed to build deb package")
endif()

message(STATUS "Created: ${DEB_PATH}")
