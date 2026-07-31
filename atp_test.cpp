#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <assert.h>
#include <vector>
#include "audio_thread_priority.h"

int main() {
  atp_thread_info* info = atp_get_current_thread_info();
  atp_thread_info* info2 = nullptr;

  // ATP_THREAD_INFO_SIZE is a runtime `extern size_t`, not a compile-time constant, so it can't
  // size a stack array portably (a VLA is a GCC/Clang extension MSVC doesn't support). Heap-
  // allocate instead.
  std::vector<uint8_t> buffer(ATP_THREAD_INFO_SIZE);
  atp_serialize_thread_info(info, buffer.data());

  info2 = atp_deserialize_thread_info(buffer.data());

  // Compare the two structs via a second serialization round-trip rather than memcmp'ing their
  // raw in-memory representations: the platform-specific atp_thread_info can contain padding
  // bytes between fields (e.g. on macOS) that aren't guaranteed to be identical between two
  // independently-constructed instances even when every actual field is equal.
  // atp_serialize_thread_info packs fields explicitly with no padding, so comparing its output is
  // well-defined.
  std::vector<uint8_t> buffer2(ATP_THREAD_INFO_SIZE);
  atp_serialize_thread_info(info2, buffer2.data());
  int rv = memcmp(buffer.data(), buffer2.data(), ATP_THREAD_INFO_SIZE);

  assert(!rv);

  atp_free_thread_info(info);
  atp_free_thread_info(info2);

#ifdef __linux__
  rv = atp_set_real_time_limit(0, 44100);
  assert(!rv);
#endif

  return 0;
}
