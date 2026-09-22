package app.lockbook.util

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class OpenLinkParserTest {
    private val id = "a6743b18-c7ef-4960-9825-8022e2fa5672"

    @Test
    fun `parses exact official web link`() {
        assertEquals(
            PublicOpenFileRequest("https://app.lockbook.net", id),
            OpenLinkParser.parse("https://app.lockbook.net/open/$id"),
        )
    }

    @Test
    fun `parses self hosted app handoff`() {
        assertEquals(
            PublicOpenFileRequest("https://notes.example.com:8443", id),
            OpenLinkParser.parse("lb://open?server=https%3A%2F%2FNotes.Example.com%3A8443%2F&file=$id"),
        )
    }

    @Test
    fun `parses legacy custom scheme link`() {
        assertEquals(
            PublicOpenFileRequest(OpenLinkParser.OFFICIAL_ORIGIN, id),
            OpenLinkParser.parse("lb://$id"),
        )
    }

    @Test
    fun `rejects unsafe and malformed links`() {
        val invalid =
            listOf(
                "http://app.lockbook.net/open/$id",
                "https://user@app.lockbook.net/open/$id",
                "https://app.lockbook.net/open/$id#fragment",
                "https://app.lockbook.net/open/$id?extra=true",
                "https://app.lockbook.net/other/$id",
                "https://app.lockbook.net/open/not-a-uuid",
                "lb://open?server=http%3A%2F%2Fnotes.example.com&file=$id",
                "lb://open?server=https%3A%2F%2Fuser%40notes.example.com&file=$id",
                "lb://open?server=https%3A%2F%2Fnotes.example.com&file=$id#fragment",
            )
        invalid.forEach { assertNull(it, OpenLinkParser.parse(it)) }
    }

    @Test
    fun `canonical origin matching ignores case slash and default port`() {
        assertTrue(OpenLinkParser.originsMatch("https://EXAMPLE.com:443/", "https://example.com"))
        assertFalse(OpenLinkParser.originsMatch("https://example.com", "https://other.example.com"))
        assertFalse(OpenLinkParser.originsMatch("http://example.com", "https://example.com"))
    }
}
