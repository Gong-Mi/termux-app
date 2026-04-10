package com.termux.terminal

/**
 * A circular byte buffer allowing one producer and one consumer thread.
 */
internal class ByteQueue(size: Int) {

    private val mBuffer = ByteArray(size)
    private var mHead = 0
    private var mStoredBytes = 0
    private var mOpen = true

    @Synchronized
    fun close() {
        mOpen = false
        notify()
    }

    @Synchronized
    fun read(buffer: ByteArray, block: Boolean): Int {
        while (mStoredBytes == 0 && mOpen) {
            if (block) {
                try { wait() } catch (_: InterruptedException) {}
            } else {
                return 0
            }
        }
        if (!mOpen) return -1

        var totalRead = 0
        val bufferLength = mBuffer.size
        val wasFull = bufferLength == mStoredBytes
        var length = buffer.size
        var offset = 0
        while (length > 0 && mStoredBytes > 0) {
            val oneRun = (bufferLength - mHead).coerceAtMost(mStoredBytes)
            val bytesToCopy = length.coerceAtMost(oneRun)
            System.arraycopy(mBuffer, mHead, buffer, offset, bytesToCopy)
            mHead += bytesToCopy
            if (mHead >= bufferLength) mHead = 0
            mStoredBytes -= bytesToCopy
            length -= bytesToCopy
            offset += bytesToCopy
            totalRead += bytesToCopy
        }
        if (wasFull) notify()
        return totalRead
    }

    /**
     * Attempt to write the specified portion of the provided buffer to the queue.
     * Returns whether the output was totally written, false if it was closed before.
     */
    fun write(buffer: ByteArray, offset: Int, lengthToWrite: Int): Boolean {
        if (lengthToWrite + offset > buffer.size) {
            throw IllegalArgumentException("length + offset > buffer.length")
        } else if (lengthToWrite <= 0) {
            throw IllegalArgumentException("length <= 0")
        }

        val bufferLength = mBuffer.size
        var remainingLength = lengthToWrite
        var currentOffset = offset

        synchronized(this) {
            while (remainingLength > 0) {
                while (bufferLength == mStoredBytes && mOpen) {
                    try { wait() } catch (_: InterruptedException) {}
                }
                if (!mOpen) return false
                val wasEmpty = mStoredBytes == 0
                var bytesToWriteBeforeWaiting = remainingLength.coerceAtMost(bufferLength - mStoredBytes)
                remainingLength -= bytesToWriteBeforeWaiting

                while (bytesToWriteBeforeWaiting > 0) {
                    var tail = mHead + mStoredBytes
                    val oneRun: Int
                    if (tail >= bufferLength) {
                        tail -= bufferLength
                        oneRun = mHead - tail
                    } else {
                        oneRun = bufferLength - tail
                    }
                    val bytesToCopy = oneRun.coerceAtMost(bytesToWriteBeforeWaiting)
                    System.arraycopy(buffer, currentOffset, mBuffer, tail, bytesToCopy)
                    currentOffset += bytesToCopy
                    bytesToWriteBeforeWaiting -= bytesToCopy
                    mStoredBytes += bytesToCopy
                }
                if (wasEmpty) notify()
            }
        }
        return true
    }
}
