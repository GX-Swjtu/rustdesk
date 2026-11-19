// ffi.kt

package ffi

import android.content.Context
import java.nio.ByteBuffer

import com.carriez.flutter_hbb.RdClipboardManager

object FFI {
    init {
        System.loadLibrary("rustdesk")
    }

    external fun onAppStart(ctx: Context)
    external fun setClipboardManager(clipboardManager: RdClipboardManager)
    external fun onAudioFrameUpdate(buf: ByteBuffer)
    external fun closeAllSessions()
    external fun initNativeP2p(ctx: Context)
    @androidx.annotation.Keep
    fun nativeP2pInterfaces(ctx: Context): String =
        com.carriez.flutter_hbb.NativeP2pNetwork.interfaces(ctx)
    external fun setFrameRawEnable(name: String, value: Boolean)
    external fun onClipboardUpdate(clips: ByteBuffer)
}
