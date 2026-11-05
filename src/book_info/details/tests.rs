use super::*;

// 2026-11-05에 알라딘 상품 페이지에서 그대로 떠온 조각이다(마크업이 바뀌면 이 테스트가 먼저 깨진다):
//   - 데이터베이스 설계, 이렇게 하면 된다(번역서, 종이책, ISBN 9791194587514)
//   - 정전(함윤이, 종이책 9791141615208 — 분류가 두 줄) / 같은 책 전자책 9791141616120
//   - 재이(조남주, 종이책 9791141603649 — 기본정보에 "양장본")
// 전자책 조각은 제목과 판본 탭만 떴다(크레딧 줄 안에 목록이 겹쳐 있어 잘라 붙이면 깨진다).

pub(crate) const DB_DESIGN_PAGE: &str = r##"<span class="Ere_bo_title">데이터베이스 설계, 이렇게 하면 된다</span> <span class="Ere_sub1_title">- 정규화부터 클라우드 대응 설계, ER 다이어그램 작성, 성능 튜닝, 안티패턴 회피, 실무에서 쓸 수 있는 그레이 노하우까지 담은 전문가의 DB 설계 비법서, 제2판</span> <a href="/search/wsearchresult.aspx?SearchTarget=All&SearchWord=9791194587514"><img src="//image.aladin.co.kr/img/shop/2018/icon_top_search.png" style="margin-bottom:-4px;" alt="검색"/></a>
</div>
<li class="Ere_sub2_title"><a href="/search/wsearchresult.aspx?AuthorSearch=%eb%af%b8%ed%81%ac@3600067&BranchType=1" class="Ere_sub2_title">미크</a>&nbsp;(지은이),<span class="Ere_PR10"></span><a href="/search/wsearchresult.aspx?AuthorSearch=%ec%9c%a4%ec%9d%b8%ec%84%b1@2318918&BranchType=1" class="Ere_sub2_title">윤인성</a>&nbsp;(옮긴이)<span class="Ere_PR10"></span><a class="Ere_sub2_title" href="/search/wsearchresult.aspx?PublisherSearch=%ec%a0%9c%ec%9d%b4%ed%8e%8d@33202&BranchType=1">제이펍</a><span class="Ere_PR10"></span>2025-10-30<span class="Ere_PR10"></span><a class="Ere_sub2_title" href="/search/wsearchresult.aspx?SearchTarget=Foreign&SearchWord=%e9%81%94%e4%ba%ba%e3%81%ab%e5%ad%b8%e3%81%b6DB%e8%a8%ad%e8%a8%88%e5%be%b9%e5%ba%95%e6%8c%87%e5%8d%97%e6%9b%b8+%e3%83%9f%e3%83%83%e3%82%af">원제 : 達人に學ぶDB設計徹底指南書</a></li>
<div class="book_tab_box"><span class="tab_on">종이책</span><span class="tab_off"><a href="/shop/wproduct.aspx?ItemId=380128880&start=pebook " >전자책 17,010원</a></span></div>
<div class="conts_info_list1"><ul><li>356쪽</li><li>170*225mm</li><li>676g</li><li>ISBN : 9791194587514</li></ul></div>
<ul id="ulCategory">
                    <li ><a href='/home/wbookmain.aspx'>국내도서</a>&nbsp;>&nbsp;<a href='/shop/wbrowse.aspx?CID=351'>컴퓨터/모바일</a>&nbsp;>&nbsp;<a href='/shop/wbrowse.aspx?CID=2719'>컴퓨터 공학</a>&nbsp;>&nbsp;<a href='/shop/wbrowse.aspx?CID=6794&BrowseTarget=List'><b>데이터베이스 개론</b></a> <a href="javascript:fn_ForeignCategoryToggle();" class="Ere_sub_gray8 Ere_fs13 foreignCategoryFold" style="display:none;">접기<img src="//image.aladin.co.kr/img/shop/2018/icon_arrow_fold.png" style="margin-bottom:-1px;"></a></li>
        
                    
        
                    

                    
                    
                </ul>"##;

pub(crate) const JEONGJEON_PAPER_PAGE: &str = r##"<span class="Ere_bo_title">정전</span> <span class="Ere_sub1_title">- 제31회 문학동네소설상 수상작</span> <a href="/search/wsearchresult.aspx?SearchTarget=All&SearchWord=9791141615208"><img src="//image.aladin.co.kr/img/shop/2018/icon_top_search.png" style="margin-bottom:-4px;" alt="검색"/></a>
</div>
<li class="Ere_sub2_title"><a href="/search/wsearchresult.aspx?AuthorSearch=%ed%95%a8%ec%9c%a4%ec%9d%b4@8774982&BranchType=1" class="Ere_sub2_title">함윤이</a>&nbsp;(지은이)<span class="Ere_PR10"></span><a class="Ere_sub2_title" href="/search/wsearchresult.aspx?PublisherSearch=%eb%ac%b8%ed%95%99%eb%8f%99%eb%84%a4@1728&BranchType=1">문학동네</a><span class="Ere_PR10"></span>2026-03-13</li>
<div class="book_tab_box"><span class="tab_on">종이책</span><span class="tab_off"><a href="/shop/wproduct.aspx?ItemId=389478983&start=pebook " >전자책 11,070원</a></span></div>
<div class="conts_info_list1"><ul><li>324쪽</li><li>133*200mm</li><li>421g</li><li>ISBN : 9791141615208</li></ul></div>
<ul id="ulCategory">
                    <li ><a href='/home/wbookmain.aspx'>국내도서</a>&nbsp;>&nbsp;<a href='/shop/wbrowse.aspx?CID=1'>소설/시/희곡</a>&nbsp;>&nbsp;<a href='/shop/wbrowse.aspx?CID=50917'>한국소설</a>&nbsp;>&nbsp;<a href='/shop/wbrowse.aspx?CID=50993&BrowseTarget=List'><b>2000년대 이후 한국소설</b></a> <a href="javascript:fn_ForeignCategoryToggle();" class="Ere_sub_gray8 Ere_fs13 foreignCategoryFold" style="display:none;">접기<img src="//image.aladin.co.kr/img/shop/2018/icon_arrow_fold.png" style="margin-bottom:-1px;"></a></li>
        
                    
        
                    <li ><a href='/home/wbookmain.aspx'>국내도서</a>&nbsp;>&nbsp;<a href='/shop/book/Recommend2010/main.aspx'>추천도서</a> > <a href='/shop/book/Recommend2010/award.aspx?awardtype=1'>국내 문학상</a> > <a href='/shop/book/Recommend2010/award.aspx?awardtype=1&awardid=10#focus' style='font-weight: bold;'>문학동네 소설상</a> <a href="javascript:fn_ForeignCategoryToggle();" class="Ere_sub_gray8 Ere_fs13 foreignCategoryFold" style="display:none;">접기<img src="//image.aladin.co.kr/img/shop/2018/icon_arrow_fold.png" style="margin-bottom:-1px;"></a></li>

                    
                    
                </ul>"##;

pub(crate) const JAEI_PAGE: &str = r##"<span class="Ere_bo_title">재이</span> <a href="/search/wsearchresult.aspx?SearchTarget=All&SearchWord=9791141603649"><img src="//image.aladin.co.kr/img/shop/2018/icon_top_search.png" style="margin-bottom:-4px;" alt="검색"/></a>
</div>
<li class="Ere_sub2_title"><a href="/search/wsearchresult.aspx?AuthorSearch=%ec%a1%b0%eb%82%a8%ec%a3%bc@2329461&BranchType=1" class="Ere_sub2_title">조남주</a>&nbsp;(지은이)<span class="Ere_PR10"></span><a class="Ere_sub2_title" href="/search/wsearchresult.aspx?PublisherSearch=%eb%ac%b8%ed%95%99%eb%8f%99%eb%84%a4@1728&BranchType=1">문학동네</a><span class="Ere_PR10"></span>2026-09-09</li>
<div class="book_tab_box"><span class="tab_on">종이책</span><span class="tab_off"><a href="/shop/wproduct.aspx?ItemId=401865654&start=pebook " >전자책 11,340원</a></span></div>
<div class="conts_info_list1"><ul><li>양장본</li><li>232쪽</li><li>128*188mm (B6)</li><li>325g</li><li>ISBN : 9791141603649</li></ul></div>
<ul id="ulCategory">
                    <li ><a href='/home/wbookmain.aspx'>국내도서</a>&nbsp;>&nbsp;<a href='/shop/wbrowse.aspx?CID=1'>소설/시/희곡</a>&nbsp;>&nbsp;<a href='/shop/wbrowse.aspx?CID=50917'>한국소설</a>&nbsp;>&nbsp;<a href='/shop/wbrowse.aspx?CID=50993&BrowseTarget=List'><b>2000년대 이후 한국소설</b></a> <a href="javascript:fn_ForeignCategoryToggle();" class="Ere_sub_gray8 Ere_fs13 foreignCategoryFold" style="display:none;">접기<img src="//image.aladin.co.kr/img/shop/2018/icon_arrow_fold.png" style="margin-bottom:-1px;"></a></li>
        
                    
        
                    

                    
                    
                </ul>"##;

pub(crate) const JEONGJEON_EBOOK_PAGE: &str = r##"<span class="Ere_bo_title">[eBook] 정전</span>
<div class="book_tab_box"><span class="tab_on">전자책</span><span class="tab_off"><a href="/shop/wproduct.aspx?ItemId=387900696&start=pbook" >종이책 15,750원</a></span></div>"##;

fn person(name: &str, role: &str) -> Person {
    Person { name: name.to_string(), role: Some(role.to_string()) }
}

#[test]
fn translated_paper_book_yields_every_field() {
    let page = ProductPage::from_html(DB_DESIGN_PAGE);
    assert!(!page.is_ebook);
    let d = page.details;
    assert_eq!(d.title.as_deref(), Some("데이터베이스 설계, 이렇게 하면 된다"));
    assert!(d.subtitle.as_deref().unwrap().starts_with("정규화부터 클라우드 대응 설계"));
    assert!(d.subtitle.as_deref().unwrap().ends_with("제2판"));
    assert_eq!(d.people, vec![person("미크", "지은이"), person("윤인성", "옮긴이")]);
    assert_eq!(d.publisher.as_deref(), Some("제이펍"));
    assert_eq!(d.pub_date.as_deref(), Some("2025-10-30"));
    assert_eq!(d.original_title.as_deref(), Some("達人に學ぶDB設計徹底指南書"));
    assert!(d.has_ebook);
    assert_eq!(d.pages, Some(356));
    assert_eq!(d.size.as_deref(), Some("170*225mm"));
    assert_eq!(d.weight.as_deref(), Some("676g"));
    assert!(d.extra_info.is_empty());
    assert_eq!(d.categories, vec!["국내도서 > 컴퓨터/모바일 > 컴퓨터 공학 > 데이터베이스 개론"]);
}

#[test]
fn every_category_line_is_kept_in_order() {
    // 정전은 장르 분류 말고 "추천도서 > 국내 문학상" 줄이 하나 더 있다(줄 사이 구분자가 &nbsp;가 아니라 그냥 " > "다).
    let d = ProductPage::from_html(JEONGJEON_PAPER_PAGE).details;
    assert_eq!(
        d.categories,
        vec![
            "국내도서 > 소설/시/희곡 > 한국소설 > 2000년대 이후 한국소설",
            "국내도서 > 추천도서 > 국내 문학상 > 문학동네 소설상",
        ],
    );
    assert_eq!(d.subtitle.as_deref(), Some("제31회 문학동네소설상 수상작"));
    assert!(d.original_title.is_none());
}

#[test]
fn unknown_basic_info_items_are_kept_and_size_may_carry_a_label() {
    let d = ProductPage::from_html(JAEI_PAGE).details;
    assert_eq!(d.pages, Some(232));
    assert_eq!(d.size.as_deref(), Some("128*188mm (B6)"));
    assert_eq!(d.weight.as_deref(), Some("325g"));
    assert_eq!(d.extra_info, vec!["양장본"]);
    assert!(d.subtitle.is_none());
}

#[test]
fn ebook_page_is_recognized_by_its_tab_and_title() {
    let page = ProductPage::from_html(JEONGJEON_EBOOK_PAGE);
    assert!(page.is_ebook);
    assert_eq!(page.details.title.as_deref(), Some("정전"), "[eBook] 접두는 뗀다");
    assert!(!page.details.has_ebook, "전자책 페이지의 다른 탭은 종이책이다");
}

#[test]
fn people_sharing_one_role_all_get_it_and_roles_are_kept_verbatim() {
    let html = r#"<li class="Ere_sub2_title"><a href="/x?AuthorSearch=a" class="Ere_sub2_title">가</a>,&nbsp;<a href="/x?AuthorSearch=b" class="Ere_sub2_title">나</a>&nbsp;(지은이),<span class="Ere_PR10"></span><a href="/x?AuthorSearch=c" class="Ere_sub2_title">다</a>&nbsp;(그림)<span class="Ere_PR10"></span><a class="Ere_sub2_title" href="/x?PublisherSearch=p">출판사</a><span class="Ere_PR10"></span>2024-01-02</li>"#;
    let d = ProductPage::from_html(html).details;
    assert_eq!(d.people, vec![person("가", "지은이"), person("나", "지은이"), person("다", "그림")]);
    assert_eq!(d.publisher.as_deref(), Some("출판사"));
    assert_eq!(d.pub_date.as_deref(), Some("2024-01-02"));
}

#[test]
fn a_person_without_a_role_is_kept_without_one() {
    let html = r#"<li class="Ere_sub2_title"><a href="/x?AuthorSearch=a">가</a><span class="Ere_PR10"></span>2024-01-02</li>"#;
    assert_eq!(
        ProductPage::from_html(html).details.people,
        vec![Person { name: "가".to_string(), role: None }],
    );
}

#[test]
fn empty_page_yields_nothing_and_is_not_an_ebook() {
    assert_eq!(ProductPage::from_html("<html></html>"), ProductPage::default());
}

#[test]
fn cover_main_image_is_read_from_its_id() {
    let html = r#"<img src="https://image.aladin.co.kr/product/1/x.jpg" id="mainCoverImg"><img id="CoverMainImage" src="https://image.aladin.co.kr/product/37500/27/cover500/k212032723_1.jpg" class="imgbox" alt="x">"#;
    assert_eq!(
        extract_cover_main_image(html).as_deref(),
        Some("https://image.aladin.co.kr/product/37500/27/cover500/k212032723_1.jpg"),
    );
    assert_eq!(extract_cover_main_image("<img src='a.jpg'>"), None);
}

#[test]
fn list_items_do_not_match_link_tags() {
    // `<li`를 찾다가 `<link`에 걸리면 안 된다.
    let html = r#"<div class="conts_info_list1"><link rel="x"><ul><li>100쪽</li></ul></div>"#;
    assert_eq!(ProductPage::from_html(html).details.pages, Some(100));
}

#[test]
fn entities_are_decoded() {
    assert_eq!(clean_text("&lt;재이&gt;&nbsp;포스터 &amp; 굿즈 &#39;a&#x27;"), "<재이> 포스터 & 굿즈 'a'");
    assert_eq!(clean_text("AT&T"), "AT&T");
}
