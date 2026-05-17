// Guided-file import worker (Phase 6.5 GM-F2). Mirrors the GTK
// shell's `guided.rs::do_import_io`: copies the source to
// `<filesDir>/meditate/guided/<uuid>.ogg` as-is when it is already
// a passthrough container (wav / ogg — same set as
// meditate_core::sound::is_passthrough_ext), otherwise transcodes
// it to Opus-in-Ogg. GTK lands Ogg/Vorbis; Android's MediaCodec
// can't encode Vorbis, so we use Opus — still Ogg, still playable
// by the GTK/gstreamer side, so a synced file round-trips. The
// reason we transcode at all (rather than copy the mp3) is the
// known "mp3 synced to a Linux device → audio problems" bug.
//
// Runs on a background Thread (a long guide is tens of MB and the
// decode/encode loop is CPU-bound — never on the UI thread).
// Result is handed back to Rust via the drop-file
// `<filesDir>/meditate/guided_import_result` ("ok" | "err:<msg>"),
// polled by src/guided.rs::take_import_result on the next tick.

package io.github.janekbt.Meditate

import android.content.Context
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMuxer
import android.os.Build
import android.util.Log
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder

object MeditateGuidedImport {
    private const val TAG = "MeditateGuided"

    // Opus is internally 48 kHz; Android's encoder is happiest there.
    // We resample decoder PCM to this rather than risk an encoder
    // that rejects 44.1 kHz (mirrors GTK's audioresample stage).
    private const val OPUS_RATE = 48_000

    @JvmStatic
    fun startImport(
        context: Context,
        src: String,
        dest: String,
        durationSecs: Long,
    ) {
        Thread {
            val dir = File(context.filesDir, "meditate")
            val progressFile = File(dir, "guided_import_progress")
            // 0 % at the start; the Rust tick poll reads this and
            // fills the Converting… button. Throttled to whole-
            // percent changes so we're not hammering the FS.
            var lastPct = -1
            val onProgress: (Int) -> Unit = { pct ->
                val p = pct.coerceIn(0, 99) // 100 == result "ok"
                if (p != lastPct) {
                    lastPct = p
                    runCatching { progressFile.writeText(p.toString()) }
                }
            }
            runCatching { progressFile.writeText("0") }
            // Cancel flag: Rust writes guided_import_cancel on the
            // Cancel tap; the transcode loop polls this and aborts.
            // Clear any stale flag from a prior run first.
            val cancelFile = File(dir, "guided_import_cancel")
            runCatching { cancelFile.delete() }
            val isCancelled: () -> Boolean = { cancelFile.exists() }
            val result = try {
                doImport(
                    src, dest, durationSecs, onProgress, isCancelled,
                )
                if (isCancelled()) {
                    runCatching { File(dest).delete() }
                    "err:cancelled"
                } else {
                    "ok"
                }
            } catch (e: Throwable) {
                Log.w(TAG, "import failed src=$src: $e")
                runCatching { File(dest).delete() }
                "err:" + (e.message ?: e.javaClass.simpleName)
            }
            // A cancelled run writes NO result: Rust already
            // dropped the finalize slot, and a late "err:cancelled"
            // would otherwise be consumed as the *next* import's
            // outcome (cross-run contamination).
            if (!isCancelled()) {
                runCatching {
                    File(dir, "guided_import_result")
                        .writeText(result)
                }
            }
        }.start()
    }

    private fun doImport(
        src: String,
        dest: String,
        durationSecs: Long,
        onProgress: (Int) -> Unit,
        isCancelled: () -> Boolean,
    ) {
        File(dest).parentFile?.mkdirs()
        val ext = src.substringAfterLast('.', "").lowercase()
        // Passthrough set matches meditate_core::sound::is_passthrough_ext
        // (wav | ogg). Everything else is transcoded.
        if (ext == "wav" || ext == "ogg") {
            if (isCancelled()) return
            File(src).copyTo(File(dest), overwrite = true)
            return
        }
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
            throw IllegalStateException("Import needs Android 10+")
        }
        transcodeToOpusOgg(
            src, dest, durationSecs, onProgress, isCancelled,
        )
    }

    private fun transcodeToOpusOgg(
        src: String,
        dest: String,
        durationSecs: Long,
        onProgress: (Int) -> Unit,
        isCancelled: () -> Boolean,
    ) {
        val extractor = MediaExtractor()
        extractor.setDataSource(src)
        var track = -1
        var inFormat: MediaFormat? = null
        for (i in 0 until extractor.trackCount) {
            val f = extractor.getTrackFormat(i)
            if (f.getString(MediaFormat.KEY_MIME)?.startsWith("audio/") == true) {
                track = i
                inFormat = f
                break
            }
        }
        if (track < 0 || inFormat == null) {
            extractor.release()
            throw IllegalStateException("no audio track")
        }
        extractor.selectTrack(track)

        val srcRate = inFormat.getInteger(MediaFormat.KEY_SAMPLE_RATE)
        val channels = inFormat.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
            .coerceIn(1, 2) // Opus encoder: mono or stereo

        // Progress denominator: prefer Rust's accurate frame-walked
        // duration (it survives headerless VBR mp3, unlike the
        // container's KEY_DURATION estimate); fall back to the
        // track header only if the probe came back 0.
        val totalUs = if (durationSecs > 0) {
            durationSecs * 1_000_000L
        } else if (inFormat.containsKey(MediaFormat.KEY_DURATION)) {
            inFormat.getLong(MediaFormat.KEY_DURATION)
        } else {
            0L
        }

        val decoder = MediaCodec.createDecoderByType(
            inFormat.getString(MediaFormat.KEY_MIME)!!,
        )
        decoder.configure(inFormat, null, null, 0)
        decoder.start()

        val encFormat = MediaFormat.createAudioFormat(
            MediaFormat.MIMETYPE_AUDIO_OPUS, OPUS_RATE, channels,
        ).apply {
            setInteger(MediaFormat.KEY_BIT_RATE, 48_000 * channels)
        }
        val encoder = MediaCodec.createEncoderByType(
            MediaFormat.MIMETYPE_AUDIO_OPUS,
        )
        encoder.configure(
            encFormat, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE,
        )
        encoder.start()

        val muxer = MediaMuxer(dest, MediaMuxer.OutputFormat.MUXER_OUTPUT_OGG)
        var muxTrack = -1
        var muxerStarted = false

        // Per-channel linear resampler state (srcRate → OPUS_RATE).
        val resampler = Resampler(srcRate, OPUS_RATE, channels)

        val info = MediaCodec.BufferInfo()
        var extractorDone = false
        var decoderDone = false
        var encoderDone = false
        val timeoutUs = 10_000L
        var ptsUs = 0L
        val bytesPerFrame = 2 * channels

        // Pull every currently-available encoder output → muxer.
        // Must be called often enough that the encoder's input
        // buffers don't all stay checked out (a classic
        // dequeueInputBuffer-forever deadlock). Handles the
        // codec-config skip + lazy muxer start on format-change.
        fun drainEncoder() {
            while (true) {
                val encOut =
                    encoder.dequeueOutputBuffer(info, 0)
                if (encOut == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                    if (!muxerStarted) {
                        muxTrack =
                            muxer.addTrack(encoder.outputFormat)
                        muxer.start()
                        muxerStarted = true
                    }
                    continue
                }
                if (encOut < 0) break // TRY_AGAIN / no output yet
                if (info.flags and
                    MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0
                ) {
                    info.size = 0 // CSD folds into the track format
                }
                if (info.size > 0 && muxerStarted) {
                    val outBuf = encoder.getOutputBuffer(encOut)!!
                    outBuf.position(info.offset)
                    outBuf.limit(info.offset + info.size)
                    muxer.writeSampleData(muxTrack, outBuf, info)
                }
                if (info.flags and
                    MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0
                ) {
                    encoderDone = true
                }
                encoder.releaseOutputBuffer(encOut, false)
                if (encoderDone) break
            }
        }

        // Feed a resampled PCM chunk into the encoder, splitting it
        // across as many input buffers as needed — an Opus input
        // buffer is far smaller than a resampled decode chunk, so
        // a single put() overflows (the BufferOverflowException
        // bug). Drain interleaved so input buffers free up.
        fun feedEncoder(pcm: ByteBuffer) {
            while (pcm.hasRemaining()) {
                val eInIx =
                    encoder.dequeueInputBuffer(timeoutUs)
                if (eInIx < 0) {
                    drainEncoder()
                    continue
                }
                val eBuf = encoder.getInputBuffer(eInIx)!!
                eBuf.clear()
                val n = minOf(pcm.remaining(), eBuf.remaining())
                val slice = pcm.slice()
                slice.limit(n)
                eBuf.put(slice)
                pcm.position(pcm.position() + n)
                encoder.queueInputBuffer(eInIx, 0, n, ptsUs, 0)
                ptsUs += if (bytesPerFrame > 0) {
                    (n / bytesPerFrame) * 1_000_000L / OPUS_RATE
                } else {
                    0L
                }
                drainEncoder()
            }
        }

        try {
            while (!encoderDone) {
                // Cancel check (cheap file stat) — bail promptly
                // when Rust flags the Cancel tap. startImport's
                // isCancelled() re-check then deletes the partial
                // dest and reports err:cancelled.
                if (isCancelled()) break
                // 1. Feed compressed input → decoder.
                if (!extractorDone) {
                    val inIx = decoder.dequeueInputBuffer(timeoutUs)
                    if (inIx >= 0) {
                        val buf = decoder.getInputBuffer(inIx)!!
                        val n = extractor.readSampleData(buf, 0)
                        if (n < 0) {
                            decoder.queueInputBuffer(
                                inIx, 0, 0, 0,
                                MediaCodec.BUFFER_FLAG_END_OF_STREAM,
                            )
                            extractorDone = true
                        } else {
                            decoder.queueInputBuffer(
                                inIx, 0, n, extractor.sampleTime, 0,
                            )
                            extractor.advance()
                        }
                    }
                }

                // 2. Drain decoder PCM → resample → encoder.
                if (!decoderDone) {
                    val outIx = decoder.dequeueOutputBuffer(info, timeoutUs)
                    if (outIx >= 0) {
                        val pcm = decoder.getOutputBuffer(outIx)!!
                        pcm.position(info.offset)
                        pcm.limit(info.offset + info.size)
                        val eos = info.flags and
                            MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0
                        val resampled = resampler.process(pcm, eos)
                        decoder.releaseOutputBuffer(outIx, false)
                        if (resampled != null && resampled.hasRemaining()) {
                            feedEncoder(resampled)
                        }
                        if (eos) {
                            // Signal end-of-stream to the encoder
                            // with an empty input buffer (its own
                            // EOS output then ends the loop).
                            var queued = false
                            while (!queued) {
                                val eInIx = encoder
                                    .dequeueInputBuffer(timeoutUs)
                                if (eInIx < 0) {
                                    drainEncoder()
                                    continue
                                }
                                encoder.queueInputBuffer(
                                    eInIx, 0, 0, ptsUs,
                                    MediaCodec
                                        .BUFFER_FLAG_END_OF_STREAM,
                                )
                                queued = true
                            }
                            decoderDone = true
                        }
                    }
                }

                // 3. Drain encoder → muxer, then report progress.
                drainEncoder()
                if (totalUs > 0) {
                    onProgress((ptsUs * 100 / totalUs).toInt())
                }
            }
        } finally {
            runCatching { if (muxerStarted) muxer.stop() }
            runCatching { muxer.release() }
            runCatching { decoder.stop() }
            runCatching { decoder.release() }
            runCatching { encoder.stop() }
            runCatching { encoder.release() }
            runCatching { extractor.release() }
        }
    }

    // Per-channel linear-interpolation resampler for 16-bit
    // interleaved PCM. Android's Opus encoder only accepts 48 kHz
    // input (Opus is internally 48 kHz), so 44.1 kHz sources MUST
    // be resampled — this is the audioresample stage of GTK's
    // gstreamer pipeline. Linear (not polyphase) is fine for
    // spoken-word + ambient guided content.
    //
    // PCM from MediaCodec is native byte order; a ByteBuffer
    // defaults to BIG_ENDIAN, so the order MUST be set before
    // asShortBuffer() or every sample is byte-swapped into noise.
    // Output is grown dynamically (no worst-case-size guesswork —
    // that risked an ArrayIndexOutOfBounds) and the per-channel
    // `prev` frame + fractional phase persist across chunks so
    // there are no clicks at chunk boundaries.
    private class Resampler(
        inRate: Int,
        private val outRate: Int,
        private val channels: Int,
    ) {
        private val step = inRate.toDouble() / outRate.toDouble()
        private val passthrough = inRate == outRate
        private val prev = ShortArray(channels)
        private var havePrev = false
        private var phase = 0.0 // output position within [prev,cur)

        fun process(pcm: ByteBuffer, eos: Boolean): ByteBuffer? {
            pcm.order(ByteOrder.nativeOrder())
            if (passthrough) {
                // Raw byte copy — endianness irrelevant, and the
                // decoder buffer is released right after, so we
                // must own a copy.
                val n = pcm.remaining()
                if (n == 0) return null
                val out = ByteBuffer.allocate(n)
                out.put(pcm)
                out.flip()
                return out
            }
            val ins = pcm.asShortBuffer()
            val inFrames = ins.remaining() / channels
            if (inFrames == 0) return null

            var outLen = 0
            var out = ShortArray(channels * (inFrames + 16))
            fun push(frame: ShortArray) {
                if (outLen + channels > out.size) {
                    out = out.copyOf(out.size * 2)
                }
                for (c in 0 until channels) out[outLen++] = frame[c]
            }

            val cur = ShortArray(channels)
            val emit = ShortArray(channels)
            if (!havePrev) {
                for (c in 0 until channels) prev[c] = ins.get(c)
                havePrev = true
            }
            for (f in 0 until inFrames) {
                for (c in 0 until channels) {
                    cur[c] = ins.get(f * channels + c)
                }
                while (phase < 1.0) {
                    for (c in 0 until channels) {
                        val a = prev[c].toDouble()
                        val b = cur[c].toDouble()
                        emit[c] = (a + (b - a) * phase)
                            .toInt().coerceIn(-32768, 32767)
                            .toShort()
                    }
                    push(emit)
                    phase += step
                }
                phase -= 1.0
                System.arraycopy(cur, 0, prev, 0, channels)
            }
            if (eos) push(prev) // don't drop the trailing frame

            if (outLen == 0) return null
            val res = ByteBuffer
                .allocate(outLen * 2)
                .order(ByteOrder.nativeOrder())
            res.asShortBuffer().put(out, 0, outLen)
            return res
        }
    }
}
