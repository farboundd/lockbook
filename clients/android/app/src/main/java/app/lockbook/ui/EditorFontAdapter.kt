package app.lockbook.ui

import android.content.Context
import android.graphics.Typeface
import android.os.Build
import android.view.View
import android.view.ViewGroup
import android.widget.ArrayAdapter
import android.widget.TextView
import app.lockbook.R
import org.json.JSONArray

data class EditorFontChoice(
    val family: String,
    val label: String,
    val typeface: Typeface,
    val faces: Map<Int, Typeface> = mapOf(400 to typeface),
) {
    val weights: List<Int> get() = faces.keys.sorted()

    fun atWeight(weight: Int): Typeface = faces.minByOrNull { kotlin.math.abs(it.key - weight) }?.value ?: typeface

    override fun toString(): String = label

    companion object {
        // Called off the UI thread: font files and TTC faces come from the editor.
        fun load(
            json: String,
            defaultLabel: String,
        ): List<EditorFontChoice> {
            val fonts = JSONArray(json)
            return (0 until fonts.length()).map { index ->
                val font = fonts.getJSONObject(index)
                val family = font.getString("family")
                val availableFaces = font.getJSONArray("faces")
                val faces =
                    (0 until availableFaces.length())
                        .associate { faceIndex ->
                            val face = availableFaces.getJSONObject(faceIndex)
                            val weight = face.getInt("weight")
                            val path = face.optString("path").takeUnless { it.isEmpty() || it == "null" }
                            val typeface =
                                runCatching {
                                    if (path == null) {
                                        Typeface.DEFAULT
                                    } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                                        Typeface
                                            .Builder(path)
                                            .setTtcIndex(face.getInt("index"))
                                            .setWeight(weight)
                                            .setFontVariationSettings("'wght' $weight")
                                            .setItalic(face.getBoolean("italic"))
                                            .build() ?: Typeface.DEFAULT
                                    } else {
                                        Typeface.createFromFile(path)
                                    }
                                }.getOrDefault(Typeface.DEFAULT)
                            weight to typeface
                        }.ifEmpty { mapOf(400 to Typeface.DEFAULT) }
                val regular = faces.minByOrNull { kotlin.math.abs(it.key - 400) }!!.value
                EditorFontChoice(family, family.ifEmpty { defaultLabel }, regular, faces)
            }
        }
    }
}

class EditorFontAdapter(
    context: Context,
    choices: List<EditorFontChoice>,
    var weight: Int,
) : ArrayAdapter<EditorFontChoice>(context, R.layout.editor_font_dropdown_item, choices) {
    override fun getView(
        position: Int,
        convertView: View?,
        parent: ViewGroup,
    ): View =
        (super.getView(position, convertView, parent) as TextView).apply {
            typeface = getItem(position)?.atWeight(weight) ?: Typeface.DEFAULT
        }

    override fun getDropDownView(
        position: Int,
        convertView: View?,
        parent: ViewGroup,
    ): View = getView(position, convertView, parent)
}
