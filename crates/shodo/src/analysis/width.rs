//! Inverse Unicode width mappings for multi-unit horizontal composition.
// UnicodeData 16.0 <wide>/<narrow> mappings; voiced Katakana use their
// canonical base+mark decomposition. No compatibility normalization is applied
// to unrelated characters (for example ligatures and superscripts).

pub(crate) fn narrow(c: char) -> ([char; 2], usize) {
    if ('\u{ff01}'..='\u{ff5e}').contains(&c) {
        return ([char::from_u32(c as u32 - 0xfee0).unwrap(), '\0'], 1);
    }
    match FORMS.binary_search_by_key(&c, |(full, _, _)| *full) {
        Ok(index) => {
            let (_, forms, count) = FORMS[index];
            (forms, count)
        }
        Err(_) => ([c, '\0'], 1),
    }
}

const FORMS: &[(char, [char; 2], usize)] = &[
    ('\u{2190}', ['\u{ffe9}', '\0'], 1),
    ('\u{2191}', ['\u{ffea}', '\0'], 1),
    ('\u{2192}', ['\u{ffeb}', '\0'], 1),
    ('\u{2193}', ['\u{ffec}', '\0'], 1),
    ('\u{2502}', ['\u{ffe8}', '\0'], 1),
    ('\u{25a0}', ['\u{ffed}', '\0'], 1),
    ('\u{25cb}', ['\u{ffee}', '\0'], 1),
    ('\u{3000}', ['\u{20}', '\0'], 1),
    ('\u{3001}', ['\u{ff64}', '\0'], 1),
    ('\u{3002}', ['\u{ff61}', '\0'], 1),
    ('\u{300c}', ['\u{ff62}', '\0'], 1),
    ('\u{300d}', ['\u{ff63}', '\0'], 1),
    ('\u{3099}', ['\u{ff9e}', '\0'], 1),
    ('\u{309a}', ['\u{ff9f}', '\0'], 1),
    ('\u{30a1}', ['\u{ff67}', '\0'], 1),
    ('\u{30a2}', ['\u{ff71}', '\0'], 1),
    ('\u{30a3}', ['\u{ff68}', '\0'], 1),
    ('\u{30a4}', ['\u{ff72}', '\0'], 1),
    ('\u{30a5}', ['\u{ff69}', '\0'], 1),
    ('\u{30a6}', ['\u{ff73}', '\0'], 1),
    ('\u{30a7}', ['\u{ff6a}', '\0'], 1),
    ('\u{30a8}', ['\u{ff74}', '\0'], 1),
    ('\u{30a9}', ['\u{ff6b}', '\0'], 1),
    ('\u{30aa}', ['\u{ff75}', '\0'], 1),
    ('\u{30ab}', ['\u{ff76}', '\0'], 1),
    ('\u{30ac}', ['\u{ff76}', '\u{ff9e}'], 2),
    ('\u{30ad}', ['\u{ff77}', '\0'], 1),
    ('\u{30ae}', ['\u{ff77}', '\u{ff9e}'], 2),
    ('\u{30af}', ['\u{ff78}', '\0'], 1),
    ('\u{30b0}', ['\u{ff78}', '\u{ff9e}'], 2),
    ('\u{30b1}', ['\u{ff79}', '\0'], 1),
    ('\u{30b2}', ['\u{ff79}', '\u{ff9e}'], 2),
    ('\u{30b3}', ['\u{ff7a}', '\0'], 1),
    ('\u{30b4}', ['\u{ff7a}', '\u{ff9e}'], 2),
    ('\u{30b5}', ['\u{ff7b}', '\0'], 1),
    ('\u{30b6}', ['\u{ff7b}', '\u{ff9e}'], 2),
    ('\u{30b7}', ['\u{ff7c}', '\0'], 1),
    ('\u{30b8}', ['\u{ff7c}', '\u{ff9e}'], 2),
    ('\u{30b9}', ['\u{ff7d}', '\0'], 1),
    ('\u{30ba}', ['\u{ff7d}', '\u{ff9e}'], 2),
    ('\u{30bb}', ['\u{ff7e}', '\0'], 1),
    ('\u{30bc}', ['\u{ff7e}', '\u{ff9e}'], 2),
    ('\u{30bd}', ['\u{ff7f}', '\0'], 1),
    ('\u{30be}', ['\u{ff7f}', '\u{ff9e}'], 2),
    ('\u{30bf}', ['\u{ff80}', '\0'], 1),
    ('\u{30c0}', ['\u{ff80}', '\u{ff9e}'], 2),
    ('\u{30c1}', ['\u{ff81}', '\0'], 1),
    ('\u{30c2}', ['\u{ff81}', '\u{ff9e}'], 2),
    ('\u{30c3}', ['\u{ff6f}', '\0'], 1),
    ('\u{30c4}', ['\u{ff82}', '\0'], 1),
    ('\u{30c5}', ['\u{ff82}', '\u{ff9e}'], 2),
    ('\u{30c6}', ['\u{ff83}', '\0'], 1),
    ('\u{30c7}', ['\u{ff83}', '\u{ff9e}'], 2),
    ('\u{30c8}', ['\u{ff84}', '\0'], 1),
    ('\u{30c9}', ['\u{ff84}', '\u{ff9e}'], 2),
    ('\u{30ca}', ['\u{ff85}', '\0'], 1),
    ('\u{30cb}', ['\u{ff86}', '\0'], 1),
    ('\u{30cc}', ['\u{ff87}', '\0'], 1),
    ('\u{30cd}', ['\u{ff88}', '\0'], 1),
    ('\u{30ce}', ['\u{ff89}', '\0'], 1),
    ('\u{30cf}', ['\u{ff8a}', '\0'], 1),
    ('\u{30d0}', ['\u{ff8a}', '\u{ff9e}'], 2),
    ('\u{30d1}', ['\u{ff8a}', '\u{ff9f}'], 2),
    ('\u{30d2}', ['\u{ff8b}', '\0'], 1),
    ('\u{30d3}', ['\u{ff8b}', '\u{ff9e}'], 2),
    ('\u{30d4}', ['\u{ff8b}', '\u{ff9f}'], 2),
    ('\u{30d5}', ['\u{ff8c}', '\0'], 1),
    ('\u{30d6}', ['\u{ff8c}', '\u{ff9e}'], 2),
    ('\u{30d7}', ['\u{ff8c}', '\u{ff9f}'], 2),
    ('\u{30d8}', ['\u{ff8d}', '\0'], 1),
    ('\u{30d9}', ['\u{ff8d}', '\u{ff9e}'], 2),
    ('\u{30da}', ['\u{ff8d}', '\u{ff9f}'], 2),
    ('\u{30db}', ['\u{ff8e}', '\0'], 1),
    ('\u{30dc}', ['\u{ff8e}', '\u{ff9e}'], 2),
    ('\u{30dd}', ['\u{ff8e}', '\u{ff9f}'], 2),
    ('\u{30de}', ['\u{ff8f}', '\0'], 1),
    ('\u{30df}', ['\u{ff90}', '\0'], 1),
    ('\u{30e0}', ['\u{ff91}', '\0'], 1),
    ('\u{30e1}', ['\u{ff92}', '\0'], 1),
    ('\u{30e2}', ['\u{ff93}', '\0'], 1),
    ('\u{30e3}', ['\u{ff6c}', '\0'], 1),
    ('\u{30e4}', ['\u{ff94}', '\0'], 1),
    ('\u{30e5}', ['\u{ff6d}', '\0'], 1),
    ('\u{30e6}', ['\u{ff95}', '\0'], 1),
    ('\u{30e7}', ['\u{ff6e}', '\0'], 1),
    ('\u{30e8}', ['\u{ff96}', '\0'], 1),
    ('\u{30e9}', ['\u{ff97}', '\0'], 1),
    ('\u{30ea}', ['\u{ff98}', '\0'], 1),
    ('\u{30eb}', ['\u{ff99}', '\0'], 1),
    ('\u{30ec}', ['\u{ff9a}', '\0'], 1),
    ('\u{30ed}', ['\u{ff9b}', '\0'], 1),
    ('\u{30ef}', ['\u{ff9c}', '\0'], 1),
    ('\u{30f2}', ['\u{ff66}', '\0'], 1),
    ('\u{30f3}', ['\u{ff9d}', '\0'], 1),
    ('\u{30f4}', ['\u{ff73}', '\u{ff9e}'], 2),
    ('\u{30f7}', ['\u{ff9c}', '\u{ff9e}'], 2),
    ('\u{30fa}', ['\u{ff66}', '\u{ff9e}'], 2),
    ('\u{30fb}', ['\u{ff65}', '\0'], 1),
    ('\u{30fc}', ['\u{ff70}', '\0'], 1),
    ('\u{3131}', ['\u{ffa1}', '\0'], 1),
    ('\u{3132}', ['\u{ffa2}', '\0'], 1),
    ('\u{3133}', ['\u{ffa3}', '\0'], 1),
    ('\u{3134}', ['\u{ffa4}', '\0'], 1),
    ('\u{3135}', ['\u{ffa5}', '\0'], 1),
    ('\u{3136}', ['\u{ffa6}', '\0'], 1),
    ('\u{3137}', ['\u{ffa7}', '\0'], 1),
    ('\u{3138}', ['\u{ffa8}', '\0'], 1),
    ('\u{3139}', ['\u{ffa9}', '\0'], 1),
    ('\u{313a}', ['\u{ffaa}', '\0'], 1),
    ('\u{313b}', ['\u{ffab}', '\0'], 1),
    ('\u{313c}', ['\u{ffac}', '\0'], 1),
    ('\u{313d}', ['\u{ffad}', '\0'], 1),
    ('\u{313e}', ['\u{ffae}', '\0'], 1),
    ('\u{313f}', ['\u{ffaf}', '\0'], 1),
    ('\u{3140}', ['\u{ffb0}', '\0'], 1),
    ('\u{3141}', ['\u{ffb1}', '\0'], 1),
    ('\u{3142}', ['\u{ffb2}', '\0'], 1),
    ('\u{3143}', ['\u{ffb3}', '\0'], 1),
    ('\u{3144}', ['\u{ffb4}', '\0'], 1),
    ('\u{3145}', ['\u{ffb5}', '\0'], 1),
    ('\u{3146}', ['\u{ffb6}', '\0'], 1),
    ('\u{3147}', ['\u{ffb7}', '\0'], 1),
    ('\u{3148}', ['\u{ffb8}', '\0'], 1),
    ('\u{3149}', ['\u{ffb9}', '\0'], 1),
    ('\u{314a}', ['\u{ffba}', '\0'], 1),
    ('\u{314b}', ['\u{ffbb}', '\0'], 1),
    ('\u{314c}', ['\u{ffbc}', '\0'], 1),
    ('\u{314d}', ['\u{ffbd}', '\0'], 1),
    ('\u{314e}', ['\u{ffbe}', '\0'], 1),
    ('\u{314f}', ['\u{ffc2}', '\0'], 1),
    ('\u{3150}', ['\u{ffc3}', '\0'], 1),
    ('\u{3151}', ['\u{ffc4}', '\0'], 1),
    ('\u{3152}', ['\u{ffc5}', '\0'], 1),
    ('\u{3153}', ['\u{ffc6}', '\0'], 1),
    ('\u{3154}', ['\u{ffc7}', '\0'], 1),
    ('\u{3155}', ['\u{ffca}', '\0'], 1),
    ('\u{3156}', ['\u{ffcb}', '\0'], 1),
    ('\u{3157}', ['\u{ffcc}', '\0'], 1),
    ('\u{3158}', ['\u{ffcd}', '\0'], 1),
    ('\u{3159}', ['\u{ffce}', '\0'], 1),
    ('\u{315a}', ['\u{ffcf}', '\0'], 1),
    ('\u{315b}', ['\u{ffd2}', '\0'], 1),
    ('\u{315c}', ['\u{ffd3}', '\0'], 1),
    ('\u{315d}', ['\u{ffd4}', '\0'], 1),
    ('\u{315e}', ['\u{ffd5}', '\0'], 1),
    ('\u{315f}', ['\u{ffd6}', '\0'], 1),
    ('\u{3160}', ['\u{ffd7}', '\0'], 1),
    ('\u{3161}', ['\u{ffda}', '\0'], 1),
    ('\u{3162}', ['\u{ffdb}', '\0'], 1),
    ('\u{3163}', ['\u{ffdc}', '\0'], 1),
    ('\u{3164}', ['\u{ffa0}', '\0'], 1),
    ('\u{ff5f}', ['\u{2985}', '\0'], 1),
    ('\u{ff60}', ['\u{2986}', '\0'], 1),
    ('\u{ffe0}', ['\u{a2}', '\0'], 1),
    ('\u{ffe1}', ['\u{a3}', '\0'], 1),
    ('\u{ffe2}', ['\u{ac}', '\0'], 1),
    ('\u{ffe3}', ['\u{af}', '\0'], 1),
    ('\u{ffe4}', ['\u{a6}', '\0'], 1),
    ('\u{ffe5}', ['\u{a5}', '\0'], 1),
    ('\u{ffe6}', ['\u{20a9}', '\0'], 1),
];

#[cfg(test)]
mod tests {
    use super::{FORMS, narrow};
    use icu_properties::{CodePointMapData, CodePointSetData, props};

    #[test]
    fn narrowing_preserves_grapheme_properties() {
        // Reusing source grapheme boundaries requires more than equal scalar
        // counts: context-sensitive emoji/Indic properties must survive too.
        let gcb = CodePointMapData::<props::GraphemeClusterBreak>::new();
        let incb = CodePointMapData::<props::IndicConjunctBreak>::new();
        let ep = CodePointSetData::new::<props::ExtendedPictographic>();
        let properties = |c| (gcb.get(c), incb.get(c), ep.contains(c));
        for c in ('\u{ff01}'..='\u{ff5e}').chain(FORMS.iter().map(|(c, _, _)| *c)) {
            let (forms, count) = narrow(c);
            assert_eq!(properties(c), properties(forms[0]), "U+{:04X}", c as u32);
            if count == 2 {
                assert_eq!(gcb.get(forms[0]), props::GraphemeClusterBreak::Other);
                assert_eq!(incb.get(forms[0]), props::IndicConjunctBreak::None);
                assert!(!ep.contains(forms[0]));
                assert_eq!(gcb.get(forms[1]), props::GraphemeClusterBreak::Extend);
            }
        }
    }
}
