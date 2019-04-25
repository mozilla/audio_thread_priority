/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this file,
 * You can obtain one at http://mozilla.org/MPL/2.0/. */

#include <cstdarg>
#include <cstdint>
#include <cstdlib>

struct atp_handle;

extern "C" {

/**
 * Promotes a thread to real-time priority.
 *
 * audio_buffer_frames: number of frames per audio buffer. If unknown, passing 0
 * will choose an appropriate number, conservatively. If variable, either pass 0
 * or an upper bound.
 * audio_samplerate_hz: sample-rate for this audio stream, in Hz
 *
 * Returns an opaque handle in case of success, NULL otherwise.
 */
atp_handle *atp_promote_current_thread_to_real_time(uint32_t audio_buffer_frames,
                                                    uint32_t audio_samplerate_hz);

/**
 * Demotes a thread promoted to real-time priority via
 * `atp_demote_current_thread_from_real_time` to its previous priority.
 *
 * Returns 0 in case of success, non-zero otherwise.
 */
int32_t atp_demote_current_thread_from_real_time(atp_handle *handle);


} // extern "C"
