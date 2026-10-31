use unicode_width::UnicodeWidthStr;

use crate::model::Alias;

pub fn added(alias: &Alias) -> String {
    format!("Added alias: {}  -->  {}\n", alias.name, alias.cmd)
}

pub fn render(aliases: &[Alias]) -> String {
    if aliases.is_empty() {
        return "No aliases found.\n".to_owned();
    }

    let rows = aliases
        .iter()
        .map(|alias| {
            [
                alias.created_at_display(),
                alias.name.clone(),
                alias.cmd.clone(),
                alias.desc.clone().unwrap_or_default(),
                alias.tags.join(", "),
            ]
        })
        .collect::<Vec<_>>();

    let headers = ["CREATED", "NAME", "COMMAND", "DESCRIPTION", "TAGS"];
    let mut widths = headers.map(UnicodeWidthStr::width);
    for row in &rows {
        for (index, value) in row.iter().enumerate() {
            widths[index] = widths[index].max(UnicodeWidthStr::width(value.as_str()));
        }
    }

    let mut output = String::new();
    write_row(&mut output, &headers, &widths);
    for row in &rows {
        let values = row.each_ref().map(String::as_str);
        write_row(&mut output, &values, &widths);
    }
    output
}

fn write_row(output: &mut String, values: &[&str; 5], widths: &[usize; 5]) {
    for (index, value) in values.iter().enumerate() {
        output.push_str(value);
        if index + 1 != values.len() {
            let padding = widths[index] - UnicodeWidthStr::width(*value) + 2;
            output.push_str(&" ".repeat(padding));
        }
    }
    output.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_unicode_descriptions() {
        let mut first = Alias::new(
            "gs",
            "git status",
            Some("查看状态".into()),
            vec!["git".into()],
        )
        .unwrap();
        first.created_at =
            Some(chrono::DateTime::parse_from_rfc3339("2026-10-30T09:24:17-07:00").unwrap());
        let aliases = [first, Alias::new("ll", "ls -lah", None, vec![]).unwrap()];
        let table = render(&aliases);
        assert!(table.contains("CREATED"));
        assert!(table.contains("2026-10-30 09:24"));
        assert!(table.contains("查看状态"));
        assert!(table.contains("git"));
    }
}
