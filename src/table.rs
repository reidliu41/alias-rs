use unicode_width::UnicodeWidthStr;

use crate::model::Alias;

pub fn render(aliases: &[Alias]) -> String {
    if aliases.is_empty() {
        return "No aliases found.\n".to_owned();
    }

    let rows = aliases
        .iter()
        .map(|alias| {
            [
                alias.name.clone(),
                alias.cmd.clone(),
                alias.desc.clone().unwrap_or_default(),
                alias.tags.join(", "),
            ]
        })
        .collect::<Vec<_>>();

    let headers = ["NAME", "COMMAND", "DESCRIPTION", "TAGS"];
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

fn write_row(output: &mut String, values: &[&str; 4], widths: &[usize; 4]) {
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
        let aliases = [
            Alias::new(
                "gs",
                "git status",
                Some("查看状态".into()),
                vec!["git".into()],
            )
            .unwrap(),
            Alias::new("ll", "ls -lah", None, vec![]).unwrap(),
        ];
        let table = render(&aliases);
        assert!(table.contains("查看状态"));
        assert!(table.contains("git"));
    }
}
