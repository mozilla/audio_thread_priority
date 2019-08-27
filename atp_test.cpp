#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <assert.h>
#include "audio_thread_priority.h"

int main() {
  atp_thread_info* info = atp_get_current_thread_info();
  atp_thread_info* info2 = nullptr;

  uint8_t buffer[ATP_THREAD_INFO_SIZE];
  atp_serialize_thread_info(info, buffer);

  info2 = atp_deserialize_thread_info(buffer);

  int rv = memcmp(info, info2, 24);

  assert(!rv);

  atp_free_thread_info(info);
  atp_free_thread_info(info2);

  return 0;
}
