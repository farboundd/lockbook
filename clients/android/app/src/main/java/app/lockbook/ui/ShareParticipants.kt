package app.lockbook.ui

import net.lockbook.File
import java.util.Locale

/**
 * Collect direct and inherited sharing participants for a file, ordered by the
 * number of files they can access through direct or inherited shares.
 */
internal fun shareParticipants(
    file: File,
    filesById: Map<String, File>,
): List<String> {
    val participants = linkedMapOf<String, String>()
    forEachShareOnPath(file, filesById) { username ->
        if (username.isNotBlank()) participants.putIfAbsent(username.lowercase(Locale.ROOT), username)
    }
    if (participants.isEmpty()) return emptyList()

    val sharedFileCounts = participants.keys.associateWith { 0 }.toMutableMap()
    filesById.values.forEach { treeFile ->
        val usersOnFile = mutableSetOf<String>()
        forEachShareOnPath(treeFile, filesById) { username ->
            val key = username.lowercase(Locale.ROOT)
            if (key in participants) usersOnFile.add(key)
        }
        usersOnFile.forEach { username -> sharedFileCounts[username] = sharedFileCounts.getValue(username) + 1 }
    }

    return participants.entries
        .sortedWith(
            compareByDescending<Map.Entry<String, String>> { sharedFileCounts.getValue(it.key) }
                .thenBy { it.key },
        ).map { it.value }
}

private inline fun forEachShareOnPath(
    file: File,
    filesById: Map<String, File>,
    visit: (String) -> Unit,
) {
    val visited = mutableSetOf<String>()
    var current: File? = file
    while (current != null && visited.add(current.id)) {
        current.shares.forEach { share ->
            visit(share.sharedBy)
            visit(share.sharedWith)
        }
        current = if (current.isRoot) null else filesById[current.parent]
    }
}
