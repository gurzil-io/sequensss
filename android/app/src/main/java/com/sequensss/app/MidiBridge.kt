package com.sequensss.app

import android.content.Context
import android.media.midi.MidiDevice
import android.media.midi.MidiDeviceInfo
import android.media.midi.MidiInputPort
import android.media.midi.MidiManager
import android.media.midi.MidiOutputPort
import android.media.midi.MidiReceiver
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * MIDI bridge between Android (`MidiManager`) and the Rust engine (crates/seq-io/src/midi_android.rs).
 *
 * Android naming: a device *sends* on its OUTPUT ports (= our inputs, "sources")
 * and *receives* on its INPUT ports (= our outputs, "destinations").
 * Every method is called from Rust through JNI, off the main thread.
 */
object MidiBridge {
    private const val TAG = "sequensss"

    private var manager: MidiManager? = null
    private val thread = HandlerThread("midi-bridge").apply { start() }
    private val handler = Handler(thread.looper)

    /** Incremented whenever a device is added or removed (read by Rust to refresh). */
    @Volatile private var gen = 0L

    private class Source(val device: MidiDevice, val port: MidiOutputPort, val receiver: Receiver)
    private class Destination(val device: MidiDevice, val port: MidiInputPort)

    private val sources = HashMap<Long, Source>()
    private val destinations = HashMap<Int, Destination>()
    private var nextId = 1

    fun init(context: Context) {
        if (manager != null) return
        val m = context.getSystemService(Context.MIDI_SERVICE) as? MidiManager
        if (m == null) {
            Log.w(TAG, "MIDI: service unavailable")
            return
        }
        manager = m
        m.registerDeviceCallback(object : MidiManager.DeviceCallback() {
            override fun onDeviceAdded(info: MidiDeviceInfo) {
                Log.i(TAG, "MIDI: connected ${deviceName(info)}")
                gen++
            }

            override fun onDeviceRemoved(info: MidiDeviceInfo) {
                Log.i(TAG, "MIDI: disconnected ${deviceName(info)}")
                gen++
            }
        }, handler)
        gen++
    }

    @JvmStatic fun generation(): Long = gen

    private fun deviceName(info: MidiDeviceInfo): String {
        val p = info.properties
        return p.getString(MidiDeviceInfo.PROPERTY_NAME)
            ?: p.getString(MidiDeviceInfo.PROPERTY_PRODUCT)
            ?: "MIDI ${info.id}"
    }

    @Suppress("DEPRECATION") // getDevicesForTransport : API 33+, minSdk 29
    private fun devices(): Array<MidiDeviceInfo> = manager?.devices ?: emptyArray()

    /** (display name, device, port number) for the ports of the requested type. */
    private fun ports(type: Int): List<Triple<String, MidiDeviceInfo, Int>> {
        val out = ArrayList<Triple<String, MidiDeviceInfo, Int>>()
        for (info in devices()) {
            val ps = info.ports.filter { it.type == type }
            for (p in ps) {
                val base = deviceName(info)
                val name = when {
                    ps.size == 1 -> base
                    p.name.isNullOrBlank() -> "$base · ${p.portNumber + 1}"
                    else -> "$base · ${p.name}"
                }
                out.add(Triple(name, info, p.portNumber))
            }
        }
        return out
    }

    @JvmStatic fun listSources(): Array<String> =
        ports(MidiDeviceInfo.PortInfo.TYPE_OUTPUT).map { it.first }.toTypedArray()

    @JvmStatic fun listDestinations(): Array<String> =
        ports(MidiDeviceInfo.PortInfo.TYPE_INPUT).map { it.first }.toTypedArray()

    /** Opens the device and waits for the result (callback delivered on our HandlerThread). */
    private fun openDevice(info: MidiDeviceInfo): MidiDevice? {
        val m = manager ?: return null
        val latch = CountDownLatch(1)
        var device: MidiDevice? = null
        m.openDevice(info, { d ->
            device = d
            latch.countDown()
        }, handler)
        latch.await(3, TimeUnit.SECONDS)
        return device
    }

    private class Receiver(val handle: Long) : MidiReceiver() {
        @Volatile var open = true

        override fun onSend(msg: ByteArray, offset: Int, count: Int, timestamp: Long) {
            // The lock guarantees that Rust does not free `handle` during a call.
            synchronized(this) {
                if (open) nativeOnMidi(handle, msg, offset, count)
            }
        }
    }

    /** null on success, otherwise the error message. */
    @JvmStatic fun openSource(name: String, handle: Long): String? {
        val (_, info, portNumber) = ports(MidiDeviceInfo.PortInfo.TYPE_OUTPUT).find { it.first == name }
            ?: return "port not found"
        val device = openDevice(info) ?: return "device refused to open"
        val port = device.openOutputPort(portNumber)
        if (port == null) {
            device.close()
            return "port already in use?"
        }
        val receiver = Receiver(handle)
        port.connect(receiver)
        synchronized(sources) { sources[handle] = Source(device, port, receiver) }
        Log.i(TAG, "MIDI in opened: $name")
        return null
    }

    @JvmStatic fun closeSource(handle: Long) {
        val s = synchronized(sources) { sources.remove(handle) } ?: return
        synchronized(s.receiver) { s.receiver.open = false }
        try {
            s.port.disconnect(s.receiver)
            s.port.close()
            s.device.close()
        } catch (e: Exception) {
            Log.w(TAG, "MIDI: close: $e")
        }
    }

    /** Handle (> 0) or -1. */
    @JvmStatic fun openDestination(name: String): Int {
        val (_, info, portNumber) = ports(MidiDeviceInfo.PortInfo.TYPE_INPUT).find { it.first == name }
            ?: return -1
        val device = openDevice(info) ?: return -1
        val port = device.openInputPort(portNumber)
        if (port == null) {
            device.close()
            return -1
        }
        val id = synchronized(destinations) {
            val id = nextId++
            destinations[id] = Destination(device, port)
            id
        }
        Log.i(TAG, "MIDI out opened: $name")
        return id
    }

    @JvmStatic fun send(id: Int, data: ByteArray): Boolean {
        val d = synchronized(destinations) { destinations[id] } ?: return false
        return try {
            d.port.send(data, 0, data.size)
            true
        } catch (e: Exception) {
            false
        }
    }

    @JvmStatic fun closeDestination(id: Int) {
        val d = synchronized(destinations) { destinations.remove(id) } ?: return
        try {
            d.port.close()
            d.device.close()
        } catch (e: Exception) {
            Log.w(TAG, "MIDI: close: $e")
        }
    }

    @JvmStatic private external fun nativeOnMidi(handle: Long, data: ByteArray, offset: Int, count: Int)
}
