use std::fmt;

/// Unicode regional indicator symbols start at U+1F1E6 (letter A).
const REGIONAL_INDICATOR_A: u32 = 0x1F1E6;

/// An ISO 3166-1 alpha-2 code, rendered as the regional indicator pair that
/// browsers and system fonts draw as a flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flag([u8; 2]);

impl fmt::Display for Flag {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      for letter in self.0 {
         let indicator = char::from_u32(REGIONAL_INDICATOR_A + u32::from(letter - b'A'))
            .expect("an alpha-2 letter offsets into the regional indicator block");
         write!(f, "{indicator}")?;
      }
      Ok(())
   }
}

/// ISO 3166-1 names and their common-name variants, lowercased and sorted by
/// code point so `flag` can binary search them.
static COUNTRIES: &[(&str, [u8; 2])] = &[
   ("afghanistan", *b"AF"),
   ("albania", *b"AL"),
   ("algeria", *b"DZ"),
   ("american samoa", *b"AS"),
   ("andorra", *b"AD"),
   ("angola", *b"AO"),
   ("anguilla", *b"AI"),
   ("antarctica", *b"AQ"),
   ("antigua and barbuda", *b"AG"),
   ("argentina", *b"AR"),
   ("armenia", *b"AM"),
   ("aruba", *b"AW"),
   ("australia", *b"AU"),
   ("austria", *b"AT"),
   ("azerbaijan", *b"AZ"),
   ("bahamas", *b"BS"),
   ("bahrain", *b"BH"),
   ("bangladesh", *b"BD"),
   ("barbados", *b"BB"),
   ("belarus", *b"BY"),
   ("belgium", *b"BE"),
   ("belize", *b"BZ"),
   ("benin", *b"BJ"),
   ("bermuda", *b"BM"),
   ("bhutan", *b"BT"),
   ("bolivia", *b"BO"),
   ("bolivia, plurinational state of", *b"BO"),
   ("bonaire, sint eustatius and saba", *b"BQ"),
   ("bosnia and herzegovina", *b"BA"),
   ("botswana", *b"BW"),
   ("bouvet island", *b"BV"),
   ("brazil", *b"BR"),
   ("british indian ocean territory", *b"IO"),
   ("brunei darussalam", *b"BN"),
   ("bulgaria", *b"BG"),
   ("burkina faso", *b"BF"),
   ("burundi", *b"BI"),
   ("cabo verde", *b"CV"),
   ("cambodia", *b"KH"),
   ("cameroon", *b"CM"),
   ("canada", *b"CA"),
   ("cayman islands", *b"KY"),
   ("central african republic", *b"CF"),
   ("chad", *b"TD"),
   ("chile", *b"CL"),
   ("china", *b"CN"),
   ("christmas island", *b"CX"),
   ("cocos (keeling) islands", *b"CC"),
   ("colombia", *b"CO"),
   ("comoros", *b"KM"),
   ("congo", *b"CG"),
   ("congo, the democratic republic of the", *b"CD"),
   ("cook islands", *b"CK"),
   ("costa rica", *b"CR"),
   ("croatia", *b"HR"),
   ("cuba", *b"CU"),
   ("curaçao", *b"CW"),
   ("cyprus", *b"CY"),
   ("czechia", *b"CZ"),
   ("côte d'ivoire", *b"CI"),
   ("denmark", *b"DK"),
   ("djibouti", *b"DJ"),
   ("dominica", *b"DM"),
   ("dominican republic", *b"DO"),
   ("ecuador", *b"EC"),
   ("egypt", *b"EG"),
   ("el salvador", *b"SV"),
   ("equatorial guinea", *b"GQ"),
   ("eritrea", *b"ER"),
   ("estonia", *b"EE"),
   ("eswatini", *b"SZ"),
   ("ethiopia", *b"ET"),
   ("falkland islands (malvinas)", *b"FK"),
   ("faroe islands", *b"FO"),
   ("fiji", *b"FJ"),
   ("finland", *b"FI"),
   ("france", *b"FR"),
   ("french guiana", *b"GF"),
   ("french polynesia", *b"PF"),
   ("french southern territories", *b"TF"),
   ("gabon", *b"GA"),
   ("gambia", *b"GM"),
   ("georgia", *b"GE"),
   ("germany", *b"DE"),
   ("ghana", *b"GH"),
   ("gibraltar", *b"GI"),
   ("greece", *b"GR"),
   ("greenland", *b"GL"),
   ("grenada", *b"GD"),
   ("guadeloupe", *b"GP"),
   ("guam", *b"GU"),
   ("guatemala", *b"GT"),
   ("guernsey", *b"GG"),
   ("guinea", *b"GN"),
   ("guinea-bissau", *b"GW"),
   ("guyana", *b"GY"),
   ("haiti", *b"HT"),
   ("heard island and mcdonald islands", *b"HM"),
   ("holy see (vatican city state)", *b"VA"),
   ("honduras", *b"HN"),
   ("hong kong", *b"HK"),
   ("hungary", *b"HU"),
   ("iceland", *b"IS"),
   ("india", *b"IN"),
   ("indonesia", *b"ID"),
   ("iran", *b"IR"),
   ("iran, islamic republic of", *b"IR"),
   ("iraq", *b"IQ"),
   ("ireland", *b"IE"),
   ("isle of man", *b"IM"),
   ("israel", *b"IL"),
   ("italy", *b"IT"),
   ("jamaica", *b"JM"),
   ("japan", *b"JP"),
   ("jersey", *b"JE"),
   ("jordan", *b"JO"),
   ("kazakhstan", *b"KZ"),
   ("kenya", *b"KE"),
   ("kiribati", *b"KI"),
   ("korea, democratic people's republic of", *b"KP"),
   ("korea, republic of", *b"KR"),
   ("kuwait", *b"KW"),
   ("kyrgyzstan", *b"KG"),
   ("lao people's democratic republic", *b"LA"),
   ("laos", *b"LA"),
   ("latvia", *b"LV"),
   ("lebanon", *b"LB"),
   ("lesotho", *b"LS"),
   ("liberia", *b"LR"),
   ("libya", *b"LY"),
   ("liechtenstein", *b"LI"),
   ("lithuania", *b"LT"),
   ("luxembourg", *b"LU"),
   ("macao", *b"MO"),
   ("madagascar", *b"MG"),
   ("malawi", *b"MW"),
   ("malaysia", *b"MY"),
   ("maldives", *b"MV"),
   ("mali", *b"ML"),
   ("malta", *b"MT"),
   ("marshall islands", *b"MH"),
   ("martinique", *b"MQ"),
   ("mauritania", *b"MR"),
   ("mauritius", *b"MU"),
   ("mayotte", *b"YT"),
   ("mexico", *b"MX"),
   ("micronesia, federated states of", *b"FM"),
   ("moldova", *b"MD"),
   ("moldova, republic of", *b"MD"),
   ("monaco", *b"MC"),
   ("mongolia", *b"MN"),
   ("montenegro", *b"ME"),
   ("montserrat", *b"MS"),
   ("morocco", *b"MA"),
   ("mozambique", *b"MZ"),
   ("myanmar", *b"MM"),
   ("namibia", *b"NA"),
   ("nauru", *b"NR"),
   ("nepal", *b"NP"),
   ("netherlands", *b"NL"),
   ("new caledonia", *b"NC"),
   ("new zealand", *b"NZ"),
   ("nicaragua", *b"NI"),
   ("niger", *b"NE"),
   ("nigeria", *b"NG"),
   ("niue", *b"NU"),
   ("norfolk island", *b"NF"),
   ("north korea", *b"KP"),
   ("north macedonia", *b"MK"),
   ("northern mariana islands", *b"MP"),
   ("norway", *b"NO"),
   ("oman", *b"OM"),
   ("pakistan", *b"PK"),
   ("palau", *b"PW"),
   ("palestine, state of", *b"PS"),
   ("panama", *b"PA"),
   ("papua new guinea", *b"PG"),
   ("paraguay", *b"PY"),
   ("peru", *b"PE"),
   ("philippines", *b"PH"),
   ("pitcairn", *b"PN"),
   ("poland", *b"PL"),
   ("portugal", *b"PT"),
   ("puerto rico", *b"PR"),
   ("qatar", *b"QA"),
   ("romania", *b"RO"),
   ("russian federation", *b"RU"),
   ("rwanda", *b"RW"),
   ("réunion", *b"RE"),
   ("saint barthélemy", *b"BL"),
   ("saint helena, ascension and tristan da cunha", *b"SH"),
   ("saint kitts and nevis", *b"KN"),
   ("saint lucia", *b"LC"),
   ("saint martin (french part)", *b"MF"),
   ("saint pierre and miquelon", *b"PM"),
   ("saint vincent and the grenadines", *b"VC"),
   ("samoa", *b"WS"),
   ("san marino", *b"SM"),
   ("sao tome and principe", *b"ST"),
   ("saudi arabia", *b"SA"),
   ("senegal", *b"SN"),
   ("serbia", *b"RS"),
   ("seychelles", *b"SC"),
   ("sierra leone", *b"SL"),
   ("singapore", *b"SG"),
   ("sint maarten (dutch part)", *b"SX"),
   ("slovakia", *b"SK"),
   ("slovenia", *b"SI"),
   ("solomon islands", *b"SB"),
   ("somalia", *b"SO"),
   ("south africa", *b"ZA"),
   ("south georgia and the south sandwich islands", *b"GS"),
   ("south korea", *b"KR"),
   ("south sudan", *b"SS"),
   ("spain", *b"ES"),
   ("sri lanka", *b"LK"),
   ("sudan", *b"SD"),
   ("suriname", *b"SR"),
   ("svalbard and jan mayen", *b"SJ"),
   ("sweden", *b"SE"),
   ("switzerland", *b"CH"),
   ("syria", *b"SY"),
   ("syrian arab republic", *b"SY"),
   ("taiwan", *b"TW"),
   ("taiwan, province of china", *b"TW"),
   ("tajikistan", *b"TJ"),
   ("tanzania", *b"TZ"),
   ("tanzania, united republic of", *b"TZ"),
   ("thailand", *b"TH"),
   ("timor-leste", *b"TL"),
   ("togo", *b"TG"),
   ("tokelau", *b"TK"),
   ("tonga", *b"TO"),
   ("trinidad and tobago", *b"TT"),
   ("tunisia", *b"TN"),
   ("turkmenistan", *b"TM"),
   ("turks and caicos islands", *b"TC"),
   ("tuvalu", *b"TV"),
   ("türkiye", *b"TR"),
   ("uganda", *b"UG"),
   ("ukraine", *b"UA"),
   ("united arab emirates", *b"AE"),
   ("united kingdom", *b"GB"),
   ("united states", *b"US"),
   ("united states minor outlying islands", *b"UM"),
   ("uruguay", *b"UY"),
   ("uzbekistan", *b"UZ"),
   ("vanuatu", *b"VU"),
   ("venezuela", *b"VE"),
   ("venezuela, bolivarian republic of", *b"VE"),
   ("viet nam", *b"VN"),
   ("vietnam", *b"VN"),
   ("virgin islands, british", *b"VG"),
   ("virgin islands, u.s.", *b"VI"),
   ("wallis and futuna", *b"WF"),
   ("western sahara", *b"EH"),
   ("yemen", *b"YE"),
   ("zambia", *b"ZM"),
   ("zimbabwe", *b"ZW"),
   ("åland islands", *b"AX"),
];

/// Colloquial English names for countries whose ISO name is a formal or
/// inverted one, plus the abbreviations a caller may pass by hand. Sorted
/// alongside `COUNTRIES`.
static ALIASES: &[(&str, [u8; 2])] = &[
   ("cape verde", *b"CV"),
   ("czech republic", *b"CZ"),
   ("democratic republic of the congo", *b"CD"),
   ("east timor", *b"TL"),
   ("great britain", *b"GB"),
   ("ivory coast", *b"CI"),
   ("korea", *b"KR"),
   ("macau", *b"MO"),
   ("palestine", *b"PS"),
   ("republic of korea", *b"KR"),
   ("republic of the congo", *b"CG"),
   ("russia", *b"RU"),
   ("swaziland", *b"SZ"),
   ("the netherlands", *b"NL"),
   ("turkey", *b"TR"),
   ("uk", *b"GB"),
   ("united states of america", *b"US"),
   ("usa", *b"US"),
   ("vatican city", *b"VA"),
];

/// Expand X's ambiguous Korea label for display.
pub fn display_name(country: &str) -> &str {
   if country.trim().eq_ignore_ascii_case("korea") {
      "South Korea"
   } else {
      country
   }
}

/// Resolve an English country name, or a bare alpha-2 code, to its flag.
/// X also reports broad regions such as "Europe", which have no flag.
pub fn flag(country: &str) -> Option<Flag> {
   let country = country.trim();
   let lowered = || country.chars().flat_map(char::to_lowercase);
   let by_name = |table: &[(&str, [u8; 2])]| {
      table
         .binary_search_by(|entry| entry.0.chars().cmp(lowered()))
         .ok()
         .map(|index| Flag(table[index].1))
   };

   by_name(COUNTRIES).or_else(|| by_name(ALIASES)).or_else(|| {
      let code = <[u8; 2]>::try_from(country.as_bytes())
         .ok()?
         .map(|letter| letter.to_ascii_uppercase());

      COUNTRIES
         .iter()
         .any(|&(_, known)| known == code)
         .then_some(Flag(code))
   })
}

#[cfg(test)]
mod tests {
   use super::flag;

   #[test]
   fn x_location_strings_resolve() {
      for (location, expected) in [
         ("United States", Some("🇺🇸")),
         ("United Kingdom", Some("🇬🇧")),
         (" UK ", Some("🇬🇧")),
         ("USA", Some("🇺🇸")),
         ("br", Some("🇧🇷")),
         ("Czechia", Some("🇨🇿")),
         ("Czech Republic", Some("🇨🇿")),
         ("Russia", Some("🇷🇺")),
         ("TÜRKIYE", Some("🇹🇷")),
         ("South Korea", Some("🇰🇷")),
         (" KOREA ", Some("🇰🇷")),
         ("North Korea", Some("🇰🇵")),
         ("Vietnam", Some("🇻🇳")),
         ("Côte d'Ivoire", Some("🇨🇮")),
         ("Ivory Coast", Some("🇨🇮")),
         ("Åland Islands", Some("🇦🇽")),
         ("Europe", None),
         ("North America", None),
         ("Southeast Asia", None),
         ("", None),
         ("ZZ", None),
         ("1A", None),
         ("🇺🇸", None),
      ] {
         let found = flag(location).map(|found| found.to_string());
         assert_eq!(found.as_deref(), expected, "{location}");
      }
   }
}
