package app.lockbook.ui

import net.lockbook.File
import org.junit.Assert.assertEquals
import org.junit.Test

class ShareParticipantsTest {
    @Test
    fun `includes direct and inherited sharers and sharees once`() {
        val root = file("root", "root", share("Owner", "Alice"))
        val folder = file("folder", "root", share("owner", "Bob"))
        val document = file("document", "folder", share("Owner", "Charlie"))

        assertEquals(
            listOf("Alice", "Owner", "Bob", "Charlie"),
            shareParticipants(document, listOf(root, folder, document).associateBy { it.id }),
        )
    }

    @Test
    fun `inherited folder shares count each descendant once`() {
        val root = file("root", "root")
        val folder = file("folder", "root", share("Owner", "Alice"))
        val first = file("first", "folder", share("Owner", "Bob"), share("Owner", "Alice"))
        val second = file("second", "folder")

        assertEquals(
            listOf("Alice", "Owner", "Bob"),
            shareParticipants(first, listOf(root, folder, first, second).associateBy { it.id }),
        )
    }

    @Test
    fun `sharing on another branch contributes to the ordering`() {
        val root = file("root", "root")
        val selected = file("selected", "root", share("Owner", "Amy"), share("Owner", "Zoe"))
        val otherFolder = file("other", "root", share("Owner", "Zoe"))
        val otherDocument = file("other-document", "other")

        assertEquals(
            listOf("Owner", "Zoe", "Amy"),
            shareParticipants(selected, listOf(root, selected, otherFolder, otherDocument).associateBy { it.id }),
        )
    }

    @Test
    fun `unshared file has no participants`() {
        val root = file("root", "root")
        val document = file("document", "root")

        assertEquals(emptyList<String>(), shareParticipants(document, listOf(root, document).associateBy { it.id }))
    }

    private fun file(
        id: String,
        parent: String,
        vararg shares: File.Share,
    ): File =
        File().apply {
            this.id = id
            this.parent = parent
            name = id
            type = File.FileType.Document
            this.shares = arrayOf(*shares)
        }

    private fun share(
        by: String,
        with: String,
    ): File.Share =
        File.Share().apply {
            sharedBy = by
            sharedWith = with
            mode = File.ShareMode.Read
        }
}
