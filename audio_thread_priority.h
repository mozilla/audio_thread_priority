/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this file,
 * You can obtain one at http://mozilla.org/MPL/2.0/. */

#ifndef AUDIO_THREAD_PRIORITY_H
#define AUDIO_THREAD_PRIORITY_H

#include <stdint.h>
#include <stdlib.h>

/**
 * An opaque structure containing information about a thread that was promoted
 * to real-time priority.
 */
struct atp_handle;
struct atp_thread_info;
extern size_t ATP_THREAD_INFO_SIZE;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * Promotes the current thread to real-time priority.
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
 * Demotes the current thread, promoted to real-time priority via
 * `atp_promote_current_thread_to_real_time`, back to its previous priority.
 *
 * Returns 0 in case of success, non-zero otherwise.
 */
int32_t atp_demote_current_thread_from_real_time(atp_handle *handle);

/**
 * Frees an atp_handle. This is useful when it is impractical to call
 * `atp_demote_current_thread_from_real_time` on the right thread. Access to the
 * handle must be synchronized externally (or the related thread must have
 * exited).
 *
 * Returns 0 in case of success, non-zero otherwise.
 */
int32_t atp_free_handle(atp_handle *handle);

/*
 * Promoting/demoting a thread other than the calling one.
 *
 * This is useful when the thread that needs to become real-time cannot
 * promote itself directly (for example because it is sandboxed), possibly
 * because it lives in another process.
 *
 * To do so:
 * - Gather information on the thread that will be promoted, by calling
 *   `atp_get_current_thread_info` on the thread itself.
 * - Serialize this info.
 * - Send over the serialized data via an IPC mechanism, if the thread that
 *   will do the promotion is in another process.
 * - Deserialize the info.
 * - Call `atp_promote_thread_to_real_time`.
 *
 * Promoting a thread other than the caller's, may require elevated privileges
 * depending on the platform (for example, Linux uses a privileged rtkit D-Bus
 * service, or requires the promoting process to be privileged when built
 * without the `dbus` feature; macOS/iOS require `task_for_pid` rights when the
 * thread lives in another process).
 */

/**
 * Promotes a thread, possibly in another process, to real-time priority.
 *
 * thread_info: info on the thread to promote, gathered with
 * `atp_get_current_thread_info()`, called on the thread itself.
 * audio_buffer_frames: number of frames per audio buffer. If unknown, passing 0
 * will choose an appropriate number, conservatively. If variable, either pass 0
 * or an upper bound.
 * audio_samplerate_hz: sample-rate for this audio stream, in Hz
 *
 * Returns an opaque handle in case of success, NULL otherwise.
 */
atp_handle *atp_promote_thread_to_real_time(atp_thread_info *thread_info);

/**
 * Demotes a thread, promoted to real-time priority via
 * `atp_promote_thread_to_real_time`, back to its previous priority.
 *
 * Returns 0 in case of success, non-zero otherwise.
 */
int32_t atp_demote_thread_from_real_time(atp_thread_info* thread_info);

/**
 * Gather information from the calling thread, to be able to promote it from
 * another thread and/or process.
 *
 * Returns a non-null pointer to an `atp_thread_info` structure in case of
 * success, to be freed later with `atp_free_thread_info`, and NULL otherwise.
 */
atp_thread_info *atp_get_current_thread_info();

/**
 * Free an `atp_thread_info` structure.
 *
 * Returns 0 in case of success, non-zero in case of error (because thread_info
 * was NULL).
 */
int32_t atp_free_thread_info(atp_thread_info *thread_info);

/**
 * Serialize an `atp_thread_info` to a byte buffer that is
 * sizeof(atp_thread_info) long.
 */
void atp_serialize_thread_info(atp_thread_info *thread_info, uint8_t *bytes);

/**
 * Deserialize a byte buffer of sizeof(atp_thread_info) to an `atp_thread_info`
 * pointer. It can be then freed using atp_free_thread_info.
 * */
atp_thread_info* atp_deserialize_thread_info(uint8_t *bytes);

#ifdef __linux__
/**
 * Set the real-time computation limit (RLIMIT_RTTIME) for the calling process.
 *
 * This is needed by the rtkit/D-Bus backend before a thread can be promoted
 * from another process, and must be called from within that process (it can be
 * done before a sandbox lockdown). Without the `dbus` feature no such limit is
 * required and this is a no-op.
 *
 * This only sets the limit. For actually promoting the thread to a real-time
 * scheduling class, see `atp_promote_thread_to_real_time`.
 */
int32_t atp_set_real_time_limit(uint32_t audio_buffer_frames,
                                uint32_t audio_samplerate_hz);

#endif // __linux__

#ifdef __cplusplus
} // extern "C"
#endif // __cplusplus

#endif // AUDIO_THREAD_PRIORITY_H
