use super::clean;

#[test]
fn the_documented_example_keeps_the_words_and_drops_the_box() {
    assert_eq!(
        clean("<｜ref｜>Title<｜/ref｜><｜box｜>[[52, 40, 948, 96]]<｜/box｜>"),
        "Title"
    );
}

#[test]
fn inline_references_keep_their_words() {
    assert_eq!(
        clean("Click <｜ref｜>OK<｜/ref｜><｜box｜>[[1, 2, 3, 4]]<｜/box｜> to go on."),
        "Click OK to go on."
    );
}

#[test]
fn layout_labels_on_their_own_line_go_with_their_coordinates() {
    let raw = "<|ref|>title<|/ref|><|det|>[[10, 20, 900, 80]]<|/det|>\n# Annual Report\n\n\
<|ref|>text<|/ref|><|det|>[[10, 100, 900, 300]]<|/det|>\nThe year went well.  \n\n\n\
<｜ref｜>image<｜/ref｜><｜det｜>[[1, 2, 3, 4]]<｜/det｜>\n";
    assert_eq!(clean(raw), "# Annual Report\n\nThe year went well.");
}

#[test]
fn other_special_tokens_go_and_look_alikes_stay() {
    assert_eq!(clean("<｜grounding｜>页眉<｜end▁of▁sentence｜>"), "页眉");
    assert_eq!(clean("a <| b |> c; x < y | z"), "a <| b |> c; x < y | z");
    assert_eq!(clean("1 <|> 2"), "1 <|> 2");
    assert_eq!(clean("a<｜box｜>[[1]]"), "a[[1]]", "没收尾的 box 只去记号");
}

#[test]
fn plain_text_is_only_tidied() {
    assert_eq!(clean("\n\n第一行  \n\n\n\n第二行\n"), "第一行\n\n第二行");
    assert_eq!(clean(""), "");
}
