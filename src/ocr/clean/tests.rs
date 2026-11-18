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

#[test]
fn a_lowercase_word_in_a_ref_without_coordinates_stays() {
    assert_eq!(
        clean("<｜ref｜>abstract<｜/ref｜>\nThis paper introduces..."),
        "abstract\nThis paper introduces..."
    );
    assert_eq!(clean("<｜ref｜>foo_bar<｜/ref｜>"), "foo_bar");
}

#[test]
fn on_a_page_whose_refs_hold_text_a_lowercase_heading_stays() {
    // DeepSeek 文档的写法：ref 里是那块的字，后面跟坐标。有一处 ref 里是正文的，这一页的 ref 都是正文。
    let raw = "<｜ref｜>Title<｜/ref｜><｜box｜>[[52, 40, 948, 96]]<｜/box｜>\n\
<｜ref｜>abstract<｜/ref｜><｜box｜>[[52, 100, 948, 120]]<｜/box｜>\n\
<｜ref｜>This paper introduces a method.<｜/ref｜><｜box｜>[[52, 130, 948, 300]]<｜/box｜>";
    assert_eq!(
        clean(raw),
        "Title\nabstract\nThis paper introduces a method."
    );
}

#[test]
fn coordinates_nested_inside_a_ref_never_show() {
    assert_eq!(
        clean("<｜ref｜>Hello<｜box｜>[[10, 20, 30, 40]]<｜/box｜><｜/ref｜>"),
        "Hello"
    );
    assert_eq!(
        clean("<|ref|>title<|det|>[[1, 2, 3, 4]]<|/det|><|/ref|>\n# Report"),
        "# Report",
        "版面类别里套着坐标的也算版面标注"
    );
    assert_eq!(
        clean("<｜ref｜>Hi<｜box｜>[[1<｜/ref｜> there"),
        "Hi[[1 there",
        "ref 里没收尾的 box 只去记号，不吃掉 ref 的收尾"
    );
}
