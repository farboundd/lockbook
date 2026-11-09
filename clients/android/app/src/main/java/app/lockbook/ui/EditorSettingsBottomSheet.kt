package app.lockbook.ui

import android.content.Context
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.ColorDrawable
import android.view.LayoutInflater
import android.view.WindowManager
import androidx.core.view.ViewCompat
import androidx.preference.PreferenceManager
import app.lockbook.R
import app.lockbook.databinding.SheetEditorSettingsBinding
import app.lockbook.workspace.Workspace
import com.google.android.material.bottomsheet.BottomSheetBehavior
import com.google.android.material.bottomsheet.BottomSheetDialog
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class EditorSettingsBottomSheet(
    context: Context,
    onStyleChanged: () -> Unit,
) : BottomSheetDialog(context) {
    private val fontScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    override fun dismiss() {
        fontScope.cancel()
        super.dismiss()
    }

    init {
        val binding = SheetEditorSettingsBinding.inflate(LayoutInflater.from(context))
        val prefs = PreferenceManager.getDefaultSharedPreferences(context)
        val fontSizeKey = context.getString(R.string.editor_font_size_key)
        val marginKey = context.getString(R.string.editor_margin_key)
        val fontFamilyKey = context.getString(R.string.editor_font_family_key)
        val fontWeightKey = context.getString(R.string.editor_font_weight_key)
        val savedWeight = prefs.getInt(fontWeightKey, 400)
        var weights = listOf(savedWeight)
        var fontsReady = false
        val lineSpacingKey = context.getString(R.string.editor_line_spacing_key)
        val defaultFont = context.getString(R.string.editor_default_font)
        val selectedFont = prefs.getString(fontFamilyKey, "").orEmpty()
        var choices =
            listOf(EditorFontChoice("", defaultFont, Typeface.DEFAULT, mapOf(savedWeight to Typeface.DEFAULT))) +
                listOf(selectedFont).filter { it.isNotEmpty() }.map {
                    EditorFontChoice(
                        it,
                        it,
                        Typeface.DEFAULT,
                        mapOf(savedWeight to Typeface.DEFAULT),
                    )
                }
        binding.fontFamily.keyListener = null
        binding.fontFamily.dropDownHeight =
            minOf((192 * context.resources.displayMetrics.density).toInt(), context.resources.displayMetrics.heightPixels / 4)

        fun weightLabel(weight: Int): String {
            val name =
                when (weight) {
                    100 -> R.string.editor_weight_thin
                    200 -> R.string.editor_weight_extra_light
                    300 -> R.string.editor_weight_light
                    400 -> R.string.editor_weight_regular
                    500 -> R.string.editor_weight_medium
                    600 -> R.string.editor_weight_semi_bold
                    700 -> R.string.editor_weight_bold
                    800 -> R.string.editor_weight_extra_bold
                    900 -> R.string.editor_weight_black
                    else -> return weight.toString()
                }
            return context.getString(R.string.editor_weight_value, context.getString(name), weight)
        }

        fun showFonts() {
            val selected = choices.firstOrNull { it.family == prefs.getString(fontFamilyKey, "").orEmpty() } ?: choices.first()
            weights = selected.weights
            val desired = prefs.getInt(fontWeightKey, 400)
            val weight = weights.minByOrNull { kotlin.math.abs(it - desired) } ?: 400
            binding.fontWeight.value = 0f
            binding.fontWeight.valueTo = maxOf(1, weights.lastIndex).toFloat()
            binding.fontWeight.value = weights.indexOf(weight).coerceAtLeast(0).toFloat()
            binding.fontWeight.isEnabled = fontsReady && weights.size > 1
            binding.fontWeight.isTickVisible = weights.size > 1
            binding.fontWeightValue.text = weightLabel(weight)
            if (fontsReady && weight != desired) {
                prefs.edit().putInt(fontWeightKey, weight).apply()
                onStyleChanged()
            }
            binding.fontFamily.setAdapter(EditorFontAdapter(context, choices, weight))
            binding.fontFamily.setText(selected.label, false)
            binding.fontFamily.typeface = selected.atWeight(weight)
        }
        showFonts()
        binding.fontFamily.setOnItemClickListener { _, _, position, _ ->
            val selected = binding.fontFamily.adapter.getItem(position) as EditorFontChoice
            prefs.edit().putString(fontFamilyKey, selected.family).apply()
            showFonts()
            onStyleChanged()
        }

        fun loadFonts() {
            fontScope.launch {
                var available = withContext(Dispatchers.IO) { Workspace.availableEditorFonts() }
                while (available == null) {
                    delay(100)
                    available = withContext(Dispatchers.IO) { Workspace.availableEditorFonts() }
                }
                val previews = withContext(Dispatchers.IO) { EditorFontChoice.load(available, defaultFont) }
                choices = (previews + choices).distinctBy { it.family }
                fontsReady = true
                showFonts()
            }
        }
        binding.fontWeight.setLabelFormatter { position -> weightLabel(weights[position.toInt().coerceIn(weights.indices)]) }
        binding.fontWeight.addOnChangeListener { _, position, fromUser ->
            if (fromUser) {
                val weight = weights[position.toInt()]
                prefs.edit().putInt(fontWeightKey, weight).apply()
                binding.fontWeightValue.text = weightLabel(weight)
                val selected = choices.firstOrNull { it.family == prefs.getString(fontFamilyKey, "").orEmpty() } ?: choices.first()
                binding.fontFamily.typeface = selected.atWeight(weight)
                (binding.fontFamily.adapter as? EditorFontAdapter)?.let { adapter ->
                    adapter.weight = weight
                    adapter.notifyDataSetChanged()
                }
                onStyleChanged()
            }
        }
        binding.lineSpacing.value = prefs.getInt(lineSpacingKey, 6).coerceIn(0, 24).toFloat()
        binding.fontSize.value = prefs.getInt(fontSizeKey, 16).coerceIn(12, 32).toFloat()
        binding.margin.value = prefs.getInt(marginKey, 32).coerceIn(15, 80).toFloat()

        fun updateLabels() {
            binding.lineSpacingValue.text =
                context.resources.getQuantityString(
                    R.plurals.editor_points,
                    binding.lineSpacing.value.toInt(),
                    binding.lineSpacing.value.toInt(),
                )
            binding.fontSizeValue.text =
                context.resources.getQuantityString(R.plurals.editor_points, binding.fontSize.value.toInt(), binding.fontSize.value.toInt())
            binding.marginValue.text =
                context.resources.getQuantityString(R.plurals.editor_points, binding.margin.value.toInt(), binding.margin.value.toInt())
        }
        updateLabels()
        binding.fontSize.addOnChangeListener { _, value, fromUser ->
            if (fromUser) {
                prefs.edit().putInt(fontSizeKey, value.toInt()).apply()
                updateLabels()
                onStyleChanged()
            }
        }
        binding.margin.addOnChangeListener { _, value, fromUser ->
            if (fromUser) {
                prefs.edit().putInt(marginKey, value.toInt()).apply()
                updateLabels()
                onStyleChanged()
            }
        }
        binding.lineSpacing.addOnChangeListener { _, value, fromUser ->
            if (fromUser) {
                prefs.edit().putInt(lineSpacingKey, value.toInt()).apply()
                updateLabels()
                onStyleChanged()
            }
        }
        setContentView(binding.root)
        ViewCompat.setAccessibilityPaneTitle(binding.root, context.getString(R.string.editor_settings))
        setOnShowListener {
            loadFonts()
            // Leave the document fully visible while its typography changes live.
            window?.setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))
            window?.clearFlags(WindowManager.LayoutParams.FLAG_DIM_BEHIND)
            window?.setDimAmount(0f)
            behavior.skipCollapsed = true
            behavior.state = BottomSheetBehavior.STATE_EXPANDED
        }
    }
}
