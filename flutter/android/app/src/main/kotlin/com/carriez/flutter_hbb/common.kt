package com.carriez.flutter_hbb

import android.media.AudioRecord
import android.media.AudioRecord.READ_BLOCKING
import android.os.Build
import androidx.annotation.RequiresApi
import java.nio.ByteBuffer

const val REQ_MICROPHONE = 401
const val REQ_EXPORT_FILE = 301
const val REQ_IMPORT_FILES = 302
const val REQ_IMPORT_DIRECTORY = 303
const val REQ_EXPORT_FILES = 304

const val PICK_IMPORT_FILES = "pick_import_files"
const val IMPORT_FILE = "import_file"
const val EXPORT_FILE = "export_file"
const val PICK_IMPORT_DIRECTORY = "pick_import_directory"
const val IMPORT_DIRECTORY = "import_directory"
const val EXPORT_FILES = "export_files"
const val GET_VALUE = "get_value"

const val KEY_IS_SUPPORT_VOICE_CALL = "KEY_IS_SUPPORT_VOICE_CALL"

fun isSupportVoiceCall(): Boolean {
    // https://developer.android.com/reference/android/media/MediaRecorder.AudioSource#VOICE_COMMUNICATION
    return Build.VERSION.SDK_INT >= Build.VERSION_CODES.R
}

class AudioReader(val bufSize: Int, private val maxFrames: Int) {
    private var currentPos = 0
    private val bufferPool: Array<ByteBuffer>

    init {
        if (maxFrames < 0 || maxFrames > 32) {
            throw Exception("Out of bounds")
        }
        if (bufSize <= 0) {
            throw Exception("Wrong bufSize")
        }
        bufferPool = Array(maxFrames) {
            ByteBuffer.allocateDirect(bufSize)
        }
    }

    private fun next() {
        currentPos++
        if (currentPos >= maxFrames) {
            currentPos = 0
        }
    }

    @RequiresApi(Build.VERSION_CODES.M)
    fun readSync(audioRecord: AudioRecord): ByteBuffer? {
        val buffer = bufferPool[currentPos]
        val res = audioRecord.read(buffer, bufSize, READ_BLOCKING)
        return if (res > 0) {
            next()
            buffer
        } else {
            null
        }
    }
}
