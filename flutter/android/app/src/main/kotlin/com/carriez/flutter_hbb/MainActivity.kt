package com.carriez.flutter_hbb

import ffi.FFI

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.ClipboardManager
import android.os.Bundle
import android.util.Log
import android.view.KeyEvent
import android.view.WindowManager
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.android.FlutterView
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import kotlin.concurrent.thread
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream


class MainActivity : FlutterActivity() {
    companion object {
        var flutterMethodChannel: MethodChannel? = null
        private var _rdClipboardManager: RdClipboardManager? = null
        val rdClipboardManager: RdClipboardManager?
            get() = _rdClipboardManager;
    }

    private val channelTag = "mChannel"
    private val logTag = "mMainActivity"
    private sealed class PendingPicker {
        data class ImportFiles(val result: MethodChannel.Result) : PendingPicker()
        data class ExportFile(val source: File, val result: MethodChannel.Result) : PendingPicker()
        data class ImportDirectory(val result: MethodChannel.Result) : PendingPicker()
        data class ExportFiles(
            val sources: List<File>,
            val rejected: Int,
            val result: MethodChannel.Result,
            val documentLogs: Boolean = false
        ) : PendingPicker()
    }

    private data class ExportSource(
        val file: File,
        val children: List<ExportSource>?
    )

    private var pendingPicker: PendingPicker? = null
    private val documentPicker by lazy { DocumentPicker(this) }

    private val audioRecordHandle = AudioRecordHandle(this)
    private var microphonePermissionResult: MethodChannel.Result? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        flutterMethodChannel = MethodChannel(
            flutterEngine.dartExecutor.binaryMessenger,
            channelTag
        )
        initFlutterChannel(flutterMethodChannel!!)
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (event.action == KeyEvent.ACTION_DOWN &&
            KeyEvent.isModifierKey(event.keyCode) && currentFocus == null) {
            // Android can drop modifier-first input when the native view has no focus.
            findViewById<FlutterView>(FlutterActivity.FLUTTER_VIEW_ID)?.requestFocus()
        }
        return super.dispatchKeyEvent(event)
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (documentPicker.onResult(requestCode, resultCode, data)) return
        if (requestCode == REQ_IMPORT_FILES) {
            val pending = pendingPicker as? PendingPicker.ImportFiles ?: return
            pendingPicker = null
            if (resultCode != Activity.RESULT_OK || data == null) {
                pending.result.success(emptyList<Map<String, String>>())
                return
            }

            val uris = linkedSetOf<Uri>()
            data.data?.let { uris.add(it) }
            data.clipData?.let { clipData ->
                for (index in 0 until clipData.itemCount) {
                    uris.add(clipData.getItemAt(index).uri)
                }
            }
            thread {
                val files = uris.map { uri ->
                    mapOf(
                        "uri" to uri.toString(),
                        "name" to (displayName(uri) ?: uri.lastPathSegment.orEmpty())
                    )
                }
                runOnUiThread { pending.result.success(files) }
            }
            return
        }
        if (requestCode == REQ_EXPORT_FILE) {
            val pending = pendingPicker as? PendingPicker.ExportFile ?: return
            pendingPicker = null
            val destination = data?.data

            if (resultCode != Activity.RESULT_OK || destination == null) {
                pending.result.success(false)
                return
            }

            thread {
                try {
                    FileInputStream(pending.source).use { input ->
                        contentResolver.openOutputStream(destination, "wt")?.use { output ->
                            input.copyTo(output)
                        } ?: throw IllegalStateException("Unable to open the selected destination")
                    }
                    runOnUiThread { pending.result.success(true) }
                } catch (e: Exception) {
                    Log.e(logTag, "Failed to export file", e)
                    runOnUiThread {
                        pending.result.error("export_failed", e.message, null)
                    }
                }
            }
            return
        }
        if (requestCode == REQ_IMPORT_DIRECTORY) {
            val pending = pendingPicker as? PendingPicker.ImportDirectory ?: return
            pendingPicker = null
            val treeUri = data?.data
            if (resultCode != Activity.RESULT_OK || treeUri == null) {
                pending.result.success(null)
                return
            }
            thread {
                val selected = mapOf(
                    "uri" to treeUri.toString(),
                    "name" to (treeDisplayName(treeUri) ?: "Imported")
                )
                runOnUiThread { pending.result.success(selected) }
            }
            return
        }
        if (requestCode == REQ_EXPORT_FILES) {
            val pending = pendingPicker as? PendingPicker.ExportFiles ?: return
            pendingPicker = null
            val treeUri = data?.data
            if (resultCode != Activity.RESULT_OK || treeUri == null) {
                pending.result.success(null)
                return
            }
            thread {
                var exported = 0
                var failed = pending.rejected
                var processed = 0
                try {
                    val sources = pending.sources.map { snapshotExportSource(it) }
                    val rootDocId = DocumentsContract.getTreeDocumentId(treeUri)
                    sources.forEach { source ->
                        val ok = source?.let {
                            copyExportSourceToTree(treeUri, rootDocId, it)
                        } ?: false
                        if (ok) exported++ else failed++
                        processed++
                    }
                } catch (e: Exception) {
                    Log.e(logTag, "Failed to export selected files", e)
                    failed += pending.sources.size - processed
                }
                if (pending.documentLogs) {
                    try {
                        exported += (application as MainApplication).documentStorage.logs.exportTo(treeUri)
                    } catch (error: Exception) {
                        Log.e(logTag, "Failed to export document logs", error)
                        failed++
                    }
                }
                runOnUiThread {
                    pending.result.success(mapOf("exported" to exported, "failed" to failed))
                }
            }
            return
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (_rdClipboardManager == null) {
            _rdClipboardManager = RdClipboardManager(getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager)
            FFI.setClipboardManager(_rdClipboardManager!!)
        }
    }

    override fun onDestroy() {
        documentPicker.close()
        Log.e(logTag, "onDestroy")
        if (isFinishing) {
            FFI.closeAllSessions()
        }
        audioRecordHandle.stop()
        microphonePermissionResult?.success(false)
        microphonePermissionResult = null
        super.onDestroy()
    }

    private fun initFlutterChannel(flutterMethodChannel: MethodChannel) {
        flutterMethodChannel.setMethodCallHandler { call, result ->
            if (documentPicker.handle(call, result)) return@setMethodCallHandler
            // make sure result will be invoked, otherwise flutter will await forever
            when (call.method) {
                "enable_soft_keyboard" -> {
                    // https://blog.csdn.net/hanye2020/article/details/105553780
                    if (call.arguments as Boolean) {
                        window.clearFlags(WindowManager.LayoutParams.FLAG_ALT_FOCUSABLE_IM)
                    } else {
                        window.addFlags(WindowManager.LayoutParams.FLAG_ALT_FOCUSABLE_IM)
                    }
                    result.success(true)

                }
                "try_sync_clipboard" -> {
                    rdClipboardManager?.syncClipboard()
                    result.success(true)
                }
                "check_permission", "get_start_on_boot_opt" -> result.success(false)
                "request_voice_call_permission" -> requestMicrophone(result)
                PICK_IMPORT_FILES -> {
                    if (pendingPicker != null) {
                        result.error("picker_in_progress", "Another document picker is already open", null)
                    } else {
                        pendingPicker = PendingPicker.ImportFiles(result)
                        try {
                            startActivityForResult(
                                Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                                    addCategory(Intent.CATEGORY_OPENABLE)
                                    type = "*/*"
                                    putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
                                },
                                REQ_IMPORT_FILES
                            )
                        } catch (e: Exception) {
                            pendingPicker = null
                            result.error("picker_unavailable", e.message, null)
                        }
                    }
                }
                IMPORT_FILE -> {
                    val arguments = call.arguments as? Map<*, *>
                    val uri = (arguments?.get("uri") as? String)?.let {
                        runCatching { Uri.parse(it) }.getOrNull()
                    }
                    val path = arguments?.get("path") as? String
                    val overwrite = arguments?.get("overwrite") as? Boolean ?: false
                    val destination = path?.let { canonicalAppScopedFile(it) }

                    if (uri?.scheme != "content") {
                        result.error("invalid_uri", "The selected document URI is invalid", null)
                    } else if (destination == null ||
                        destination.isDirectory ||
                        destination.parentFile?.isDirectory != true) {
                        result.error("invalid_destination", "The destination is outside app-scoped storage", null)
                    } else {
                        thread {
                            var temporary: File? = null
                            var reservedDestination = false
                            var errorCode = "import_failed"
                            try {
                                val temporaryFile = File.createTempFile(
                                    ".rustdesk-import-",
                                    ".tmp",
                                    destination.parentFile
                                )
                                temporary = temporaryFile
                                contentResolver.openInputStream(uri)?.use { input ->
                                    FileOutputStream(temporaryFile).use { output ->
                                        input.copyTo(output)
                                    }
                                } ?: throw IllegalStateException("Unable to open the selected document")
                                if (!overwrite) {
                                    reservedDestination = destination.createNewFile()
                                    if (!reservedDestination) {
                                        throw IllegalStateException("The destination already exists")
                                    }
                                }
                                if (!temporaryFile.renameTo(destination)) {
                                    if (reservedDestination) {
                                        destination.delete()
                                    }
                                    errorCode = "rename_failed"
                                    throw IllegalStateException("Unable to replace the destination")
                                }
                                runOnUiThread { result.success(true) }
                            } catch (e: Exception) {
                                Log.e(logTag, "Failed to import file", e)
                                runOnUiThread {
                                    result.error(errorCode, e.message, null)
                                }
                            } finally {
                                temporary?.delete()
                            }
                        }
                    }
                }
                EXPORT_FILE -> {
                    val path = (call.arguments as? Map<*, *>)?.get("path") as? String
                    val source = path?.let { canonicalExportSource(it) }

                    if (source?.isFile != true) {
                        result.error("invalid_source", "The file is outside app-scoped storage", null)
                    } else if (pendingPicker != null) {
                        result.error("picker_in_progress", "Another document picker is already open", null)
                    } else {
                        val mimeType = MimeTypeMap.getSingleton()
                            .getMimeTypeFromExtension(source.extension.lowercase())
                            ?: "application/octet-stream"
                        pendingPicker = PendingPicker.ExportFile(source, result)
                        try {
                            startActivityForResult(
                                Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                                    addCategory(Intent.CATEGORY_OPENABLE)
                                    type = mimeType
                                    putExtra(Intent.EXTRA_TITLE, source.name)
                                },
                                REQ_EXPORT_FILE
                            )
                        } catch (e: Exception) {
                            pendingPicker = null
                            result.error("picker_unavailable", e.message, null)
                        }
                    }
                }
                PICK_IMPORT_DIRECTORY -> {
                    if (pendingPicker != null) {
                        result.error("picker_in_progress", "Another document picker is already open", null)
                    } else {
                        pendingPicker = PendingPicker.ImportDirectory(result)
                        try {
                            startActivityForResult(
                                Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
                                    putExtra(Intent.EXTRA_TITLE, "Select the folder to import")
                                },
                                REQ_IMPORT_DIRECTORY
                            )
                        } catch (e: Exception) {
                            pendingPicker = null
                            result.error("picker_unavailable", e.message, null)
                        }
                    }
                }
                IMPORT_DIRECTORY -> {
                    val arguments = call.arguments as? Map<*, *>
                    val uri = (arguments?.get("uri") as? String)?.let {
                        runCatching { Uri.parse(it) }.getOrNull()
                    }
                    val path = arguments?.get("path") as? String
                    val overwrite = arguments?.get("overwrite") as? Boolean ?: false
                    val destination = path?.let { canonicalAppScopedFile(it) }

                    if (uri?.scheme != "content") {
                        result.error("invalid_uri", "The selected document URI is invalid", null)
                    } else if (destination == null ||
                        destination.parentFile?.isDirectory != true ||
                        (destination.exists() && !destination.isDirectory)) {
                        result.error("invalid_destination", "The destination is outside app-scoped storage", null)
                    } else {
                        thread {
                            var temporary: File? = null
                            var backup: File? = null
                            val ok = try {
                                val parent = destination.parentFile
                                    ?: throw IllegalStateException("The destination has no parent")
                                temporary = File.createTempFile(
                                    ".rustdesk-import-dir-",
                                    ".tmp",
                                    parent
                                ).also {
                                    if (!it.delete() || !it.mkdir()) {
                                        throw IllegalStateException("Unable to create a temporary folder")
                                    }
                                }
                                if (!copyDocumentTreeToFile(uri, temporary!!)) {
                                    throw IllegalStateException("Unable to read all folder contents")
                                }
                                if (destination.exists()) {
                                    if (!overwrite) {
                                        throw IllegalStateException("The destination already exists")
                                    }
                                    val backupFile = File.createTempFile(
                                        ".rustdesk-import-backup-",
                                        ".tmp",
                                        parent
                                    )
                                    if (!backupFile.delete()) {
                                        throw IllegalStateException("Unable to prepare the destination backup")
                                    }
                                    backup = backupFile
                                    if (!destination.renameTo(backupFile)) {
                                        throw IllegalStateException("Unable to replace the destination")
                                    }
                                }
                                if (!temporary!!.renameTo(destination)) {
                                    val destinationBackup = backup
                                    if (destinationBackup != null &&
                                        !destinationBackup.renameTo(destination)
                                    ) {
                                        throw IllegalStateException(
                                            "Unable to move the imported folder and restore " +
                                                "the destination from $destinationBackup"
                                        )
                                    }
                                    throw IllegalStateException("Unable to move the imported folder")
                                }
                                temporary = null
                                val destinationBackup = backup
                                if (destinationBackup != null &&
                                    !destinationBackup.deleteRecursively()
                                ) {
                                    throw IllegalStateException(
                                        "Unable to remove the destination backup: $destinationBackup"
                                    )
                                }
                                backup = null
                                true
                            } catch (e: Exception) {
                                Log.e(logTag, "Failed to import directory", e)
                                false
                            } finally {
                                temporary?.deleteRecursively()
                            }
                            runOnUiThread { result.success(ok) }
                        }
                    }
                }
                EXPORT_FILES -> {
                    val paths = (call.arguments as? Map<*, *>)?.get("paths") as? List<*>
                    val documentLogs = (call.arguments as? Map<*, *>)?.get("documentLogs") == true
                    if (paths == null || (paths.isEmpty() && !documentLogs)) {
                        result.error("invalid_source", "The selected files are outside app-scoped storage", null)
                    } else {
                        val sources = paths.mapNotNull {
                            (it as? String)?.let(::canonicalExportSource)
                        }
                        val rejected = paths.size - sources.size
                        if (sources.isEmpty() && (!documentLogs ||
                            !(application as MainApplication).documentStorage.logs.hasPermission())) {
                            result.success(mapOf("exported" to 0, "failed" to rejected))
                        } else if (pendingPicker != null) {
                            result.error("picker_in_progress", "Another document picker is already open", null)
                        } else {
                            pendingPicker = PendingPicker.ExportFiles(
                                sources, rejected, result, documentLogs = documentLogs)
                            try {
                                startActivityForResult(
                                    Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
                                        putExtra(Intent.EXTRA_TITLE, "Select the destination folder")
                                    },
                                    REQ_EXPORT_FILES
                                )
                            } catch (e: Exception) {
                                pendingPicker = null
                                result.error("picker_unavailable", e.message, null)
                            }
                        }
                    }
                }
                GET_VALUE -> {
                    if (call.arguments is String) {
                        if (call.arguments == KEY_IS_SUPPORT_VOICE_CALL) {
                            result.success(isSupportVoiceCall())
                        } else {
                            result.error("-1", "No such key", null)
                        }
                    } else {
                        result.success(null)
                    }
                }
                "on_voice_call_started" -> {
                    result.success(audioRecordHandle.start())
                }
                "on_voice_call_closed" -> {
                    audioRecordHandle.stop()
                    result.success(true)
                }
                else -> {
                    result.error("-1", "No such method", null)
                }
            }
        }
    }

    private fun canonicalAppScopedFile(path: String): File? {
        val file = runCatching { File(path).canonicalFile }.getOrNull() ?: return null
        val allowedRoots = listOfNotNull(filesDir, getExternalFilesDir(null)).mapNotNull {
            runCatching { it.canonicalFile }.getOrNull()
        }
        return file.takeIf { candidate ->
            allowedRoots.any { root ->
                candidate == root || candidate.path.startsWith(root.path + File.separator)
            }
        }
    }

    private fun canonicalExportSource(path: String): File? {
        val original = File(path).absoluteFile
        val canonical = canonicalAppScopedFile(path) ?: return null
        return canonical.takeIf {
            original.path == canonical.path && (canonical.isFile || canonical.isDirectory)
        }
    }

    private fun snapshotExportSource(source: File): ExportSource? {
        val safeSource = canonicalExportSource(source.path) ?: return null
        if (safeSource.isFile) return ExportSource(safeSource, null)
        val sourceChildren = safeSource.listFiles() ?: return null
        val children = ArrayList<ExportSource>(sourceChildren.size)
        for (child in sourceChildren) {
            val snapshot = snapshotExportSource(child) ?: return null
            children.add(snapshot)
        }
        return ExportSource(safeSource, children)
    }

    private fun copyExportSourceToTree(
        treeUri: Uri,
        parentDocId: String,
        source: ExportSource
    ): Boolean {
        val children = source.children
        return if (children == null) {
            copyFileToTree(treeUri, parentDocId, source.file)
        } else {
            copyDirToTree(treeUri, parentDocId, source)
        }
    }

    private fun treeDisplayName(treeUri: Uri): String? {
        return try {
            val rootDocId = DocumentsContract.getTreeDocumentId(treeUri)
            val docUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, rootDocId)
            contentResolver.query(
                docUri,
                arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME),
                null,
                null,
                null
            )?.use { cursor -> if (cursor.moveToFirst()) cursor.getString(0) else null }
        } catch (e: Exception) {
            Log.w(logTag, "Failed to read selected folder name", e)
            null
        }
    }

    private fun copyDocumentTreeToFile(treeUri: Uri, destinationDir: File): Boolean {
        val rootDocId = DocumentsContract.getTreeDocumentId(treeUri)
        return copyChildrenToFile(treeUri, rootDocId, destinationDir)
    }

    private fun copyChildrenToFile(
        treeUri: Uri,
        parentDocId: String,
        destinationDir: File
    ): Boolean {
        val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentDocId)
        var ok = true
        val destinationNames = HashSet<String>()
        val cursor = contentResolver.query(childrenUri, childColumns, null, null, null)
            ?: return false
        cursor.use {
            while (cursor.moveToNext()) {
                val docId = cursor.getString(0)
                val name = cursor.getString(1)
                val mime = cursor.getString(2)
                val docUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, docId)
                if (name != null && !destinationNames.add(name)) {
                    ok = false
                    continue
                }
                val destination = safeDestinationChild(destinationDir, name)
                if (destination == null || destination.exists()) {
                    ok = false
                    continue
                }
                if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                    if (!destination.mkdirs() && !destination.isDirectory) {
                        ok = false
                        continue
                    }
                    if (!copyChildrenToFile(treeUri, docId, destination)) {
                        ok = false
                    }
                } else if (!copyDocumentToFile(docUri, destination)) {
                    ok = false
                }
            }
        }
        return ok
    }

    private fun safeDestinationChild(destinationDir: File, name: String?): File? {
        if (name.isNullOrEmpty() || name == "." || name == ".." ||
            name.indexOf('\u0000') >= 0 || name.contains('/') || name.contains('\\')) {
            return null
        }
        val parent = runCatching { destinationDir.canonicalFile }.getOrNull() ?: return null
        val child = runCatching { File(parent, name).canonicalFile }.getOrNull() ?: return null
        return child.takeIf { it.path.startsWith(parent.path + File.separator) }
    }

    private fun copyDocumentToFile(uri: Uri, destination: File): Boolean {
        return try {
            destination.parentFile?.mkdirs()
            if (destination.exists() && !destination.delete()) {
                return false
            }
            contentResolver.openInputStream(uri)?.use { input ->
                FileOutputStream(destination).use { output -> input.copyTo(output) }
            } != null
        } catch (e: Exception) {
            Log.e(logTag, "Failed to copy document to $destination", e)
            false
        }
    }

    private fun copyFileToTree(treeUri: Uri, parentDocId: String, source: File): Boolean {
        val safeSource = canonicalExportSource(source.path)?.takeIf { it.isFile } ?: return false
        return try {
            val mime = MimeTypeMap.getSingleton()
                .getMimeTypeFromExtension(safeSource.extension.lowercase())
                ?: "application/octet-stream"
            val parentUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, parentDocId)
            val docUri = DocumentsContract.createDocument(
                contentResolver,
                parentUri,
                mime,
                safeSource.name
            ) ?: return false
            contentResolver.openOutputStream(docUri, "wt")?.use { output ->
                FileInputStream(safeSource).use { input -> input.copyTo(output) }
            } ?: return false
            true
        } catch (e: Exception) {
            Log.e(logTag, "Failed to export file $safeSource", e)
            false
        }
    }

    private fun copyDirToTree(
        treeUri: Uri,
        parentDocId: String,
        source: ExportSource
    ): Boolean {
        val children = source.children ?: return false
        val safeSource = canonicalExportSource(source.file.path)?.takeIf { it.isDirectory }
            ?: return false
        val parentUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, parentDocId)
        var dirDocId = findChildDocId(treeUri, parentDocId, safeSource.name)
        if (dirDocId == null) {
            dirDocId = try {
                DocumentsContract.createDocument(
                    contentResolver,
                    parentUri,
                    DocumentsContract.Document.MIME_TYPE_DIR,
                    safeSource.name
                )?.let { DocumentsContract.getDocumentId(it) }
            } catch (e: Exception) {
                Log.e(logTag, "Failed to create folder ${safeSource.name}", e)
                null
            }
        }
        if (dirDocId == null) return false

        var ok = true
        children.forEach { child ->
            val childOk = copyExportSourceToTree(treeUri, dirDocId, child)
            if (!childOk) ok = false
        }
        return ok
    }

    private fun findChildDocId(treeUri: Uri, parentDocId: String, name: String): String? {
        val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentDocId)
        val cursor = contentResolver.query(childrenUri, childColumns, null, null, null)
            ?: throw IllegalStateException("Unable to query destination folder")
        cursor.use {
            while (cursor.moveToNext()) {
                if (cursor.getString(1) == name &&
                    cursor.getString(2) == DocumentsContract.Document.MIME_TYPE_DIR
                ) {
                    return cursor.getString(0)
                }
            }
        }
        return null
    }

    private val childColumns = arrayOf(
        DocumentsContract.Document.COLUMN_DOCUMENT_ID,
        DocumentsContract.Document.COLUMN_DISPLAY_NAME,
        DocumentsContract.Document.COLUMN_MIME_TYPE
    )

    private fun displayName(uri: Uri): String? {
        return try {
            contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
                if (cursor.moveToFirst()) cursor.getString(0) else null
            }
        } catch (e: Exception) {
            Log.w(logTag, "Failed to read selected document name", e)
            null
        }
    }

    private fun requestMicrophone(result: MethodChannel.Result) {
        val permission = android.Manifest.permission.RECORD_AUDIO
        if (!isSupportVoiceCall() || microphonePermissionResult != null) {
            result.success(false)
        } else if (androidx.core.content.ContextCompat.checkSelfPermission(this, permission) ==
            android.content.pm.PackageManager.PERMISSION_GRANTED) {
            result.success(true)
        } else {
            microphonePermissionResult = result
            androidx.core.app.ActivityCompat.requestPermissions(this, arrayOf(permission), REQ_MICROPHONE)
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == REQ_MICROPHONE) {
            microphonePermissionResult?.success(grantResults.firstOrNull() ==
                android.content.pm.PackageManager.PERMISSION_GRANTED)
            microphonePermissionResult = null
        }
    }
}
