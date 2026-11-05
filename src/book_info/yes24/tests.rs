use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use poem::{
    get, handler,
    http::StatusCode,
    listener::TcpAcceptor,
    web::{Data, Path},
    EndpointExt, Request, Response, Route, Server,
};
use sqlx::{query, query_scalar, Row, SqlitePool};

use super::*;
use crate::book_info::tests::isbn;
use crate::db::init_db;

// 2026-11-05에 "한국 장편소설" 신상품순 1쪽(CategoryProductContents)에서 그대로 떠온 책 세 권
// (언어의 악마 — 부제·POD / 일일 반출록 — 부제 없음 / 아르고스 큰글자도서 — 수상 부제).
pub(crate) const RECENT_ITEMS: &str = r##"<li class="" data-goods-no="199810641" data-iy-no="0" data-statgb="02" data-delvbadge="BOOK" >
    <div class="itemUnit">
        <div class="item_img">
            <div class="img_canvas">
                <span class="img_item">
                    <span class="img_grp">
                                                                                                <a href="/product/goods/199810641" class="lnk_img" onclick=" ">
                            <em class="img_bdr">
                                <img class="lazy" data-original="https://image.yes24.com/goods/199810641/L" src="https://image.yes24.com/momo/Noimg_L.jpg" border="0" alt="언어의 악마">
                            </em>
                        </a>
                    </span>
                </span>
            </div>
            <div class="img_btn">
                        <a href="javascript:yes24GU.openPreviewCheck(199810641); " class="btnC btn_preview"><span class="bWrap"><em class="txt">미리보기</em></span></a>
                                            </div>
        </div>
        <div class="item_info">
                <div class="info_row info_keynote">
                </div>
            <div class="info_row info_name">
                                <a class="gd_name" href="/product/goods/199810641" onclick=" ">언어의 악마</a>
                    <span class="gd_nameE">나는, 언어의 악마가 되었다</span>
                                    <span class="gd_feature">
                        [
<span class="feature">POD도서</span>                        ]
                    </span>
                <a href="/product/goods/199810641" target="_blank" class="bgYUI ico_nWin" onclick=" ">언어의 악마 새창이동</a>
            </div>
                <div class="info_row info_pubGrp">
                            <span class="authPub info_auth" onclick="">
                                <a href="https://www.yes24.com/product/search?domain=ALL&query=%25EB%25B0%2595%25EB%25AF%25BC%25EC%25A4%2580&authorNo=549378&author=박민준" target="_blank">박민준</a> 저
                            </span>
                        <span class="authPub info_pub" onclick=""><a href="https://www.yes24.com/product/search?&domain=ALL&company=%ea%b8%80ego&query=%25EA%25B8%2580ego">글ego</a></span>
                                            <span class="authPub info_date">2026년 12월</span>
                </div>
                                                        <div class="info_row info_price">
                        <strong class="txt_num"><em class="yes_b">14,000</em>원</strong>
                        <span class="txt_num dash"><em class="yes_m">14,000</em>원</span>
                                            </div>
                    <div class="info_row info_deli" name="delvTextArea"></div>                    
                                                                            <div class="info_row info_signature">
                    POD도서
                </div>
                                                        <div class="info_row info_tag">
                                <span class="tag ">
                                    <a href="https://www.yes24.com/product/search?domain=ALL&amp;query=%25EC%25B2%25AD%25EB%2585%2584%25ED%258C%25A8%25EC%258A%25A4&amp;hashNm=%ec%b2%ad%eb%85%84%ed%8c%a8%ec%8a%a4" onclick=" setTagClickExtraCode('100', '청년패스', '199810641','9203');">#청년패스</a>
                                </span>
                    </div>
                    <div class="info_row info_read">
                        초 자연 현상, 언어의 악마를 자칭하는 은발의 여성, 그리고 언어 그 자체를 무기로 쓰는 세상. 그런 곳에서 주인공은 내일을 향한다.
                    </div>
                                        <div class="info_row info_relG">
                                    </div>
                    </div>
            <div class="item_btnCol">
            <span class="btn_row">
                <span class="chkBox" style=""><label><input type="checkbox" name="ORD_GOODS_CHKBOX" id="ordChk_199810641" data-goodsno="199810641" class="basic" style=""><span class="bgYUI chk"></span></label></span>
                    <span class="numBox">
                        <span class="yesIpt ipt_wSizeF">
                            <input type="text" name="ORD_GOODS_CNT" id="ordCnt_199810641" style="ime-mode:disabled !important" title="수량설정" value="1" class="ac yes_m" onkeyup="return checkNumeric(this);" maxlength="3">
                        </span>
                        <button type="button" class="minus" onclick="order_payment.downOrderCount('199810641'); "><span class="bgYUI">수량감소</span></button>
                        <button type="button" class="plus" onclick="order_payment.upOrderCount('199810641'); "><span class="bgYUI">수량증가</span></button>
                    </span>
            </span>
                <a href="javascript:void(0);" onclick="order_payment.addCartV3('199810641', '', this, '', '', '', '', 'Search', true); " class="btnC btn_blue"><span class="bWrap"><em class="txt">카트에 넣기</em></span></a>
            <a href="javascript:void(0);" onclick="order_payment.orderDirectV3('199810641', '', this, '', '', '', '', 'Search'); " class="btnC btn_sBlue"><span class="bWrap"><em class="txt">바로구매</em></span></a>
        <a href="javascript:void(0);" class="btnC" name="btnList" onclick="order_payment.addMyListV3('199810641', '', '', true, ''); ;"><span class="bWrap"><em class="txt">리스트에 넣기</em></span></a>
<input type="hidden" name="ORD_GOODS_OPT" id="ordOpt_199810641" value="{&quot;goods_no&quot;:199810641,&quot;goods_seq&quot;:1,&quot;order_limit_yn&quot;:&quot;N&quot;,&quot;order_remain_count&quot;:0,&quot;event_no&quot;:0,&quot;add_cart_yn&quot;:&quot;Y&quot;,&quot;goods_state&quot;:&quot;02&quot;,&quot;order_limit_count&quot;:0,&quot;resource_key&quot;:&quot;01&quot;,&quot;limit_age_yn&quot;:&quot;N&quot;,&quot;limit_age&quot;:0,&quot;member_age&quot;:0,&quot;goods_name&quot;:&quot;언어의 악마&quot;,&quot;noint_quotamonth&quot;:0,&quot;min_cnt&quot;:0,&quot;max_cnt&quot;:0,&quot;opt_salepr&quot;:0,&quot;opt_yn&quot;:&quot;N&quot;,&quot;opt_inst_yn&quot;:&quot;N&quot;,&quot;flat_rate_yn&quot;:null,&quot;rent_goods_yn&quot;:&quot;N&quot;,&quot;bookclue_yn&quot;:&quot;N&quot;,&quot;goods_gb&quot;:&quot;01&quot;,&quot;goodsSortNo&quot;:&quot;001033&quot;,&quot;goodsSortNm&quot;:&quot;소설/시/희곡&quot;,&quot;goodsAuth&quot;:&quot;&lt;박민준&gt; 저&quot;,&quot;shopPrice&quot;:14000.00,&quot;salePrice&quot;:14000.00,&quot;discountShopPrice&quot;:0.00}" />
            </div>
    </div>
</li>
<li class="" data-goods-no="199519618" data-iy-no="0" data-statgb="01" data-delvbadge="BOOK" >
    <div class="itemUnit">
        <div class="item_img">
            <div class="img_canvas">
                <span class="img_item">
                    <span class="img_grp">
                                                                                                <a href="/product/goods/199519618" class="lnk_img" onclick=" ">
                            <em class="img_bdr">
                                <img class="lazy" data-original="https://image.yes24.com/goods/199519618/L" src="https://image.yes24.com/momo/Noimg_L.jpg" border="0" alt="일일 반출록">
                            </em>
                        </a>
                    </span>
                </span>
            </div>
            <div class="img_btn">
                                            </div>
        </div>
        <div class="item_info">
                <div class="info_row info_keynote">
    <span id="spanGdKeynote" class="gd_keynote">
                            <span class='iconC reserv'><em class='txt'>예약판매</em></span>
                                                        <a href="javascript:void(0);" onclick="openUrl('/product/category/series/001001046001002?SeriesNumber=358964','Pcode','003_001')" class="lnk_series">듣는 소설-02</a>
        <!-- 클래스24 상품일 경우 -->
    </span>
    <script type="text/javascript">
        if ($('#spanGdKeynote').children().length == 0) {
            $('#spanGdKeynote').remove();
        }
    </script>
                </div>
            <div class="info_row info_name">
                                <a class="gd_name" href="/product/goods/199519618" onclick=" ">일일 반출록</a>
                                <a href="/product/goods/199519618" target="_blank" class="bgYUI ico_nWin" onclick=" ">일일 반출록 새창이동</a>
            </div>
                <div class="info_row info_pubGrp">
                            <span class="authPub info_auth" onclick="">
                                <a href="https://www.yes24.com/product/search?domain=ALL&query=%25EC%2584%25B1%25ED%2595%25B4%25EB%2582%2598&authorNo=253719&author=성해나" target="_blank">성해나</a> 저
                            </span>
                        <span class="authPub info_pub" onclick=""><a href="https://www.yes24.com/product/search?&domain=ALL&company=%eb%ac%b4%ec%a0%9c&query=%25EB%25AC%25B4%25EC%25A0%259C">무제</a></span>
                                            <span class="authPub info_date">2026년 11월</span>
                </div>
                                                        <div class="info_row info_price">
                            <span class="txt_sale"><em class="num">10</em>%</span>
                        <strong class="txt_num"><em class="yes_b">15,300</em>원</strong>
                        <span class="txt_num dash"><em class="yes_m">17,000</em>원</span>
                                                    <span class="yPoint"><em class="bgYUI ico_point">포인트적립</em>850원</span>
                    </div>
                            <div class="info_row info_rating ">
                        <span class="rating_grade">
				            <span class="star rating">평균 별점</span><em class="yes_b">10.0</em>
                        </span>
                        <span class="rating_rvCount">
				            <a href="https://www.yes24.com/product/goods/199519618?ReviewYn=Y" onclick=""><em class="bit">회원리뷰</em>(<em class="txC_blue">1</em>)</a>
				            <span class="moreRatingArea">
                                <span class="moreRatingBtn">
                                        <a href="javascript:void(0);" onclick="toggleLiCont(this,$('.sGLi'),event);" class="bgYUI">정보 더 보기/감추기</a>
                                </span>
                                <span class="moreRatingLi">
                                    <span class="moreRatingLiRow">
                                        <ul class="yesAlertLi">
                                            <li><em class="bl_dot bgYUI">&nbsp;</em>종이책 리뷰 (0건)</li>
                                            <li><em class="bl_dot bgYUI">&nbsp;</em>eBook 리뷰 (0건)</li>
                                            <li><em class="bl_dot bgYUI">&nbsp;</em>종이책 한줄평 (1건)</li>
                                            <li><em class="bl_dot bgYUI">&nbsp;</em>eBook 한줄평 (0건)</li>
                                            <li><em class="bl_dot bgYUI">&nbsp;</em>종이책 사락 별점 (0건)</li>
                                            <li><em class="bl_dot bgYUI">&nbsp;</em>eBook 사락 별점 (0건)</li>
                                        </ul>
                                    </span>
                                </span>
                            </span>
                        </span>
                                                    <span class="saleNum">
                                    판매지수 35,793
                                </span>
                </div>
                    <div class="info_row info_deli" name="delvTextArea"></div>                    
                                                                                                                    <div class="info_row info_tag">
                                <span class="tag ">
                                    <a href="https://www.yes24.com/product/search?domain=ALL&amp;query=%25ED%2595%259C%25EA%25B5%25AD%25EB%25AC%25B8%25ED%2595%2599%25EC%259D%2598%25EB%25AF%25B8%25EB%259E%2598%25EA%25B0%2580%25EB%2590%25A0%25EC%25A0%258A%25EC%259D%2580%25EC%259E%2591%25EA%25B0%2580&amp;hashNm=%ed%95%9c%ea%b5%ad%eb%ac%b8%ed%95%99%ec%9d%98%eb%af%b8%eb%9e%98%ea%b0%80%eb%90%a0%ec%a0%8a%ec%9d%80%ec%9e%91%ea%b0%80" onclick=" setTagClickExtraCode('100', '한국문학의미래가될젊은작가', '199519618','8868');">#한국문학의미래가될젊은작가</a>
                                </span>
                                <span class="tag ">
                                    <a href="https://www.yes24.com/product/search?domain=ALL&amp;query=%25EA%25B4%2580%25EA%25B3%2584%25EC%259D%2598%25ED%2598%2595%25ED%2583%259C&amp;hashNm=%ea%b4%80%ea%b3%84%ec%9d%98%ed%98%95%ed%83%9c" onclick=" setTagClickExtraCode('100', '관계의형태', '199519618','5323');">#관계의형태</a>
                                </span>
                                <span class="tag ">
                                    <a href="https://www.yes24.com/product/search?domain=ALL&amp;query=%25EC%259D%25B8%25EC%2583%259D%25EC%259D%25B4%25ED%259E%2598%25EB%2593%25A4%25EB%2595%258C&amp;hashNm=%ec%9d%b8%ec%83%9d%ec%9d%b4%ed%9e%98%eb%93%a4%eb%95%8c" onclick=" setTagClickExtraCode('100', '인생이힘들때', '199519618','3637');">#인생이힘들때</a>
                                </span>
                                <span class="tag ">
                                    <a href="https://www.yes24.com/product/search?domain=ALL&amp;query=%25EC%25B2%25AD%25EB%2585%2584%25ED%258C%25A8%25EC%258A%25A4&amp;hashNm=%ec%b2%ad%eb%85%84%ed%8c%a8%ec%8a%a4" onclick=" setTagClickExtraCode('100', '청년패스', '199519618','9203');">#청년패스</a>
                                </span>
                    </div>
                    <div class="info_row info_read">
                        40만 독자가 선택한 오늘의 작가『혼모노』 성해나의 다음 챕터, 무제 듣는 소설!*황정민, 서현진, 침착맨, 서현철, 김준, 박명신, 이동휘, 문혜인고아성, 권해효, 송인성, 조현철, 박정민, 정재형, 김민수 참여*최...
                    </div>
                                        <div class="info_row info_event">
                                <div class="event">
                                        <span class="iconC freeD"><em class="txt">이벤트</em></span>
                                                                                                            <span class="iconC"><em class="txt">사은품</em></span>
<span class="iconC"><em class="txt">기획전</em></span>
                                    <span class="txt"><a href="https://event.yes24.com/earlyReader?eventNo=277397">이달의 책 11월 : 산리오 캐릭터즈 런치백</a></span>
                                    <span class="date">(26.11.01 ~ 26.11.30)</span>
                                </div>
                            </div>
                                        <div class="info_row info_relG">
                                    </div>
                    </div>
            <div class="item_btnCol">
            <span class="btn_row">
                <span class="chkBox" style=""><label><input type="checkbox" name="ORD_GOODS_CHKBOX" id="ordChk_199519618" data-goodsno="199519618" class="basic" style=""><span class="bgYUI chk"></span></label></span>
                    <span class="numBox">
                        <span class="yesIpt ipt_wSizeF">
                            <input type="text" name="ORD_GOODS_CNT" id="ordCnt_199519618" style="ime-mode:disabled !important" title="수량설정" value="1" class="ac yes_m" onkeyup="return checkNumeric(this);" maxlength="3">
                        </span>
                        <button type="button" class="minus" onclick="order_payment.downOrderCount('199519618'); "><span class="bgYUI">수량감소</span></button>
                        <button type="button" class="plus" onclick="order_payment.upOrderCount('199519618'); "><span class="bgYUI">수량증가</span></button>
                    </span>
            </span>
                <a href="javascript:void(0);" onclick="order_payment.addCartV3('199519618', '', this, '', '', '', '', 'Search', true); " class="btnC btn_blue"><span class="bWrap"><em class="txt">카트에 넣기</em></span></a>
            <a href="javascript:void(0);" onclick="order_payment.orderDirectV3('199519618', '', this, '', '', '', '', 'Search'); " class="btnC btn_sBlue"><span class="bWrap"><em class="txt">바로구매</em></span></a>
        <a href="javascript:void(0);" class="btnC" name="btnList" onclick="order_payment.addMyListV3('199519618', '', '', true, ''); ;"><span class="bWrap"><em class="txt">리스트에 넣기</em></span></a>
<input type="hidden" name="ORD_GOODS_OPT" id="ordOpt_199519618" value="{&quot;goods_no&quot;:199519618,&quot;goods_seq&quot;:1,&quot;order_limit_yn&quot;:&quot;N&quot;,&quot;order_remain_count&quot;:0,&quot;event_no&quot;:0,&quot;add_cart_yn&quot;:&quot;Y&quot;,&quot;goods_state&quot;:&quot;01&quot;,&quot;order_limit_count&quot;:0,&quot;resource_key&quot;:&quot;01&quot;,&quot;limit_age_yn&quot;:&quot;N&quot;,&quot;limit_age&quot;:0,&quot;member_age&quot;:0,&quot;goods_name&quot;:&quot;일일 반출록&quot;,&quot;noint_quotamonth&quot;:0,&quot;min_cnt&quot;:0,&quot;max_cnt&quot;:0,&quot;opt_salepr&quot;:0,&quot;opt_yn&quot;:&quot;N&quot;,&quot;opt_inst_yn&quot;:&quot;N&quot;,&quot;flat_rate_yn&quot;:null,&quot;rent_goods_yn&quot;:&quot;N&quot;,&quot;bookclue_yn&quot;:&quot;N&quot;,&quot;goods_gb&quot;:&quot;01&quot;,&quot;goodsSortNo&quot;:&quot;001033&quot;,&quot;goodsSortNm&quot;:&quot;소설/시/희곡&quot;,&quot;goodsAuth&quot;:&quot;&lt;성해나&gt; 저&quot;,&quot;shopPrice&quot;:17000.00,&quot;salePrice&quot;:15300.00,&quot;discountShopPrice&quot;:1700.00}" />
            </div>
    </div>
</li>
<li class="" data-goods-no="199886408" data-iy-no="0" data-statgb="02" data-delvbadge="BOOK" >
    <div class="itemUnit">
        <div class="item_img">
            <div class="img_canvas">
                <span class="img_item">
                    <span class="img_grp">
                                                                                                <a href="/product/goods/199886408" class="lnk_img" onclick=" ">
                            <em class="img_bdr">
                                <img class="lazy" data-original="https://image.yes24.com/goods/199886408/L" src="https://image.yes24.com/momo/Noimg_L.jpg" border="0" alt="아르고스 (큰글자도서)">
                            </em>
                        </a>
                    </span>
                </span>
            </div>
            <div class="img_btn">
                                            </div>
        </div>
        <div class="item_info">
                <div class="info_row info_keynote">
    <span id="spanGdKeynote" class="gd_keynote">
                                                                        <a href="javascript:void(0);" onclick="openUrl('/product/category/series/001001046001002?SeriesNumber=303370','Pcode','003_001')" class="lnk_series">리더스원 큰글자도서</a>
        <!-- 클래스24 상품일 경우 -->
    </span>
    <script type="text/javascript">
        if ($('#spanGdKeynote').children().length == 0) {
            $('#spanGdKeynote').remove();
        }
    </script>
                </div>
            <div class="info_row info_name">
                                <a class="gd_name" href="/product/goods/199886408" onclick=" ">아르고스 (큰글자도서)</a>
                    <span class="gd_nameE">제31회 한겨레문학상 수상작</span>
                                <a href="/product/goods/199886408" target="_blank" class="bgYUI ico_nWin" onclick=" ">아르고스 (큰글자도서) 새창이동</a>
            </div>
                <div class="info_row info_pubGrp">
                            <span class="authPub info_auth" onclick="">
                                <a href="https://www.yes24.com/product/search?domain=ALL&query=%25EC%259D%25B4%25EC%258A%25B9%25ED%2598%2595&authorNo=529500&author=이승형" target="_blank">이승형</a> 저
                            </span>
                        <span class="authPub info_pub" onclick=""><a href="https://www.yes24.com/product/search?&domain=ALL&company=%ed%95%9c%ea%b2%a8%eb%a0%88%ec%b6%9c%ed%8c%90&query=%25ED%2595%259C%25EA%25B2%25A8%25EB%25A0%2588%25EC%25B6%259C%25ED%258C%2590">한겨레출판</a></span>
                                            <span class="authPub info_date">2026년 11월</span>
                </div>
                                                        <div class="info_row info_price">
                        <strong class="txt_num"><em class="yes_b">29,000</em>원</strong>
                        <span class="txt_num dash"><em class="yes_m">29,000</em>원</span>
                                            </div>
                    <div class="info_row info_deli" name="delvTextArea"></div>                    
                                                                                                                    <div class="info_row info_tag">
                                <span class="tag ">
                                    <a href="https://www.yes24.com/product/search?domain=ALL&amp;query=%25EC%25B2%25AD%25EB%2585%2584%25ED%258C%25A8%25EC%258A%25A4&amp;hashNm=%ec%b2%ad%eb%85%84%ed%8c%a8%ec%8a%a4" onclick=" setTagClickExtraCode('100', '청년패스', '199886408','9203');">#청년패스</a>
                                </span>
                    </div>
                    <div class="info_row info_read">
                        심사위원 압도적 지지!2026년 제31회 한겨레문학상 수상작“보이지 않는 손이 잠을 빼앗아간다”누군가의 잠을 동력 삼아 결코 멈추지 않는 컨베이어 벨트……이 시대 가장 긴급한 질문을 던지는 두렵고도 통렬한 문...
                    </div>
                                        <div class="info_row info_relG">
                                    </div>
                    </div>
            <div class="item_btnCol">
            <span class="btn_row">
                <span class="chkBox" style=""><label><input type="checkbox" name="ORD_GOODS_CHKBOX" id="ordChk_199886408" data-goodsno="199886408" class="basic" style=""><span class="bgYUI chk"></span></label></span>
                    <span class="numBox">
                        <span class="yesIpt ipt_wSizeF">
                            <input type="text" name="ORD_GOODS_CNT" id="ordCnt_199886408" style="ime-mode:disabled !important" title="수량설정" value="1" class="ac yes_m" onkeyup="return checkNumeric(this);" maxlength="3">
                        </span>
                        <button type="button" class="minus" onclick="order_payment.downOrderCount('199886408'); "><span class="bgYUI">수량감소</span></button>
                        <button type="button" class="plus" onclick="order_payment.upOrderCount('199886408'); "><span class="bgYUI">수량증가</span></button>
                    </span>
            </span>
                <a href="javascript:void(0);" onclick="order_payment.addCartV3('199886408', '', this, '', '', '', '', 'Search', true); " class="btnC btn_blue"><span class="bWrap"><em class="txt">카트에 넣기</em></span></a>
            <a href="javascript:void(0);" onclick="order_payment.orderDirectV3('199886408', '', this, '', '', '', '', 'Search'); " class="btnC btn_sBlue"><span class="bWrap"><em class="txt">바로구매</em></span></a>
        <a href="javascript:void(0);" class="btnC" name="btnList" onclick="order_payment.addMyListV3('199886408', '', '', true, ''); ;"><span class="bWrap"><em class="txt">리스트에 넣기</em></span></a>
<input type="hidden" name="ORD_GOODS_OPT" id="ordOpt_199886408" value="{&quot;goods_no&quot;:199886408,&quot;goods_seq&quot;:1,&quot;order_limit_yn&quot;:&quot;N&quot;,&quot;order_remain_count&quot;:0,&quot;event_no&quot;:0,&quot;add_cart_yn&quot;:&quot;Y&quot;,&quot;goods_state&quot;:&quot;02&quot;,&quot;order_limit_count&quot;:0,&quot;resource_key&quot;:&quot;01&quot;,&quot;limit_age_yn&quot;:&quot;N&quot;,&quot;limit_age&quot;:0,&quot;member_age&quot;:0,&quot;goods_name&quot;:&quot;아르고스 (큰글자도서)&quot;,&quot;noint_quotamonth&quot;:0,&quot;min_cnt&quot;:0,&quot;max_cnt&quot;:0,&quot;opt_salepr&quot;:0,&quot;opt_yn&quot;:&quot;N&quot;,&quot;opt_inst_yn&quot;:&quot;N&quot;,&quot;flat_rate_yn&quot;:null,&quot;rent_goods_yn&quot;:&quot;N&quot;,&quot;bookclue_yn&quot;:&quot;N&quot;,&quot;goods_gb&quot;:&quot;01&quot;,&quot;goodsSortNo&quot;:&quot;001033&quot;,&quot;goodsSortNm&quot;:&quot;소설/시/희곡&quot;,&quot;goodsAuth&quot;:&quot;&lt;이승형&gt; 저&quot;,&quot;shopPrice&quot;:29000.00,&quot;salePrice&quot;:29000.00,&quot;discountShopPrice&quot;:0.00}" />
            </div>
    </div>
</li>
"##;

// ── 순수 함수 ─────────────────────────────────────────────────────────────────

#[test]
fn list_items_are_read_from_the_real_fragment() {
    let items = parse_list(RECENT_ITEMS);
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0],
        ListItem {
            goods_no: 199810641,
            title: "언어의 악마".to_string(),
            subtitle: Some("나는, 언어의 악마가 되었다".to_string()),
            author: Some("박민준 저".to_string()),
            publisher: Some("글ego".to_string()),
            pub_month: Some("2026-12".to_string()),
        },
    );
    assert_eq!(items[1].title, "일일 반출록");
    assert_eq!(items[1].subtitle, None);
    assert_eq!(items[1].publisher.as_deref(), Some("무제"));
    assert_eq!(items[1].pub_month.as_deref(), Some("2026-11"));
    assert_eq!(items[2].subtitle.as_deref(), Some("제31회 한겨레문학상 수상작"));
}

#[test]
fn list_is_empty_without_items() {
    assert!(parse_list("<html></html>").is_empty());
}

#[test]
fn month_is_normalized() {
    assert_eq!(month_of("2013년 04월").as_deref(), Some("2013-04"));
    assert_eq!(month_of("1999년 12월").as_deref(), Some("1999-12"));
    assert_eq!(month_of("2013년 13월"), None);
    assert_eq!(month_of("2013-04"), None);
}

#[test]
fn isbn_comes_from_the_books_meta_and_must_be_valid() {
    // 2026-11-05에 상품 페이지(일일 반출록, 상품번호 199519618)에서 그대로 뜬 줄
    let real = r#"<meta property="books:isbn" content="9791199709058" />"#;
    assert_eq!(parse_isbn(real).as_deref(), Some("9791199709058"));
    assert_eq!(parse_isbn(r#"<meta property="books:isbn" content="9791199709059" />"#), None);
    assert_eq!(parse_isbn("<html></html>"), None);
}

#[test]
fn only_target_publishers_match() {
    for name in ["창비", "(주)문학동네", "㈜민음사", "문학과지성사", "은행나무출판사", "열림원 "] {
        assert!(publisher_matches(name), "{name}");
    }
    for name in ["창비교육", "무제", "글ego", "문학동네어린이", ""] {
        assert!(!publisher_matches(name), "{name}");
    }
}

// ── 수집 작업 (가짜 YES24 서버) ────────────────────────────────────────────────

/// (상품번호, 출판사, "YYYY년 MM월")
type Item = (i64, &'static str, &'static str);

#[derive(Clone)]
enum Goods {
    Isbn(String),
    NoIsbn,
    Status(u16),
}

#[derive(Default)]
struct Fake {
    /// (분류, 쪽) → 책들. 없는 쪽은 빈 목록이다.
    pages: HashMap<(String, i64), Vec<Item>>,
    goods: HashMap<i64, Goods>,
    list_status: Option<u16>,
    requests: Vec<String>,
}

type Shared = Arc<Mutex<Fake>>;

fn list_html(items: &[Item]) -> String {
    items
        .iter()
        .map(|(no, publisher, month)| {
            format!(
                r#"<li class="" data-goods-no="{no}"><div class="itemUnit"><a class="gd_name" href="/product/goods/{no}">책 {no}</a>
                <span class="authPub info_auth"><a href="/x">작가</a> 저</span>
                <span class="authPub info_pub" onclick=""><a href="/y">{publisher}</a></span>
                <span class="authPub info_date">{month}</span></div></li>"#
            )
        })
        .collect()
}

#[handler]
fn fake_list(req: &Request, fake: Data<&Shared>) -> Response {
    let query = req.uri().query().unwrap_or_default().to_string();
    let param = |name: &str| {
        query.split('&').find_map(|kv| kv.strip_prefix(&format!("{name}="))).unwrap_or_default().to_string()
    };
    let mut fake = fake.lock().unwrap();
    fake.requests.push(format!("list {} {}", param("dispNo"), param("page")));
    if let Some(code) = fake.list_status {
        return Response::builder().status(StatusCode::from_u16(code).unwrap()).finish();
    }
    assert_eq!(param("order"), "RECENT");
    let items = fake.pages.get(&(param("dispNo"), param("page").parse().unwrap())).cloned().unwrap_or_default();
    Response::builder().body(list_html(&items))
}

#[handler]
fn fake_goods(Path(no): Path<i64>, fake: Data<&Shared>) -> Response {
    let mut fake = fake.lock().unwrap();
    fake.requests.push(format!("goods {no}"));
    match fake.goods.get(&no).cloned().unwrap_or(Goods::NoIsbn) {
        Goods::Isbn(isbn) => Response::builder().body(format!(r#"<meta property="books:isbn" content="{isbn}" />"#)),
        Goods::NoIsbn => Response::builder().body("<html></html>"),
        Goods::Status(code) => Response::builder().status(StatusCode::from_u16(code).unwrap()).finish(),
    }
}

struct Fixture {
    pool: SqlitePool,
    source: Yes24Source,
    fake: Shared,
}

async fn fixture() -> Fixture {
    let fake: Shared = Arc::default();
    let app = Route::new()
        .at("/product/category/CategoryProductContents", get(fake_list))
        .at("/product/goods/:no", get(fake_goods))
        .data(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(Server::new_with_acceptor(TcpAcceptor::from_tokio(listener).unwrap()).run(app));
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    init_db(&pool).await.unwrap();
    Fixture { pool, source: Yes24Source::new(&format!("http://127.0.0.1:{port}"), "gotolib-test").unwrap(), fake }
}

impl Fixture {
    fn page(&self, category: &str, page: i64, items: &[Item]) {
        self.fake.lock().unwrap().pages.insert((category.to_string(), page), items.to_vec());
    }
    fn goods(&self, no: i64, reply: Goods) {
        self.fake.lock().unwrap().goods.insert(no, reply);
    }
    async fn step(&self, now: i64) -> Step {
        process_next(&self.pool, &self.source, now).await
    }
    /// Idle이 될 때까지(최대 n번) 돈다.
    async fn drain(&self, now: i64, n: usize) {
        for _ in 0..n {
            if self.step(now).await == Step::Idle {
                return;
            }
        }
        panic!("{n}번 안에 끝나지 않았다");
    }
    fn requests(&self) -> Vec<String> {
        std::mem::take(&mut self.fake.lock().unwrap().requests)
    }
    async fn scan(&self, category: &str, column: &str) -> Option<i64> {
        query_scalar(&format!("SELECT {column} FROM yes24_scan WHERE category = ?"))
            .bind(category)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
    async fn book(&self, no: i64) -> Option<(String, Option<String>, String)> {
        query("SELECT form, isbn13, status FROM yes24_books WHERE goods_no = ?")
            .bind(no)
            .fetch_optional(&self.pool)
            .await
            .unwrap()
            .map(|row| (row.get("form"), row.get("isbn13"), row.get("status")))
    }
}

#[tokio::test]
async fn backfill_keeps_only_target_publishers_and_fetches_their_isbns_first() {
    let f = fixture().await;
    f.page(CATEGORY_NOVEL, 1, &[(1, "창비", "2026년 11월"), (2, "무제", "2026년 11월"), (3, "(주)문학동네", "2026년 10월")]);
    f.page(CATEGORY_NOVEL, 2, &[(4, "민음사", "2026년 09월")]);
    f.goods(1, Goods::Isbn(isbn(1)));
    f.goods(3, Goods::Isbn(isbn(3)));
    f.goods(4, Goods::Isbn(isbn(4)));

    f.drain(1_000, 50).await;
    // 1쪽 → 그 쪽 대상 출판사 책들의 상품 페이지 → 2쪽 → … 순서다(대상이 아닌 2번은 상품 페이지도 안 본다)
    let novel_requests: Vec<String> = f.requests().into_iter().filter(|r| !r.contains(CATEGORY_SHORT)).collect();
    assert_eq!(
        novel_requests,
        vec![
            format!("list {CATEGORY_NOVEL} 1"),
            "goods 1".to_string(),
            "goods 3".to_string(),
            format!("list {CATEGORY_NOVEL} 2"),
            "goods 4".to_string(),
            format!("list {CATEGORY_NOVEL} 3"),
        ],
    );
    assert_eq!(f.book(1).await, Some((FORM_NOVEL.to_string(), Some(isbn(1)), STATUS_OK.to_string())));
    assert_eq!(f.book(2).await, None, "대상 출판사가 아니면 저장하지 않는다");
    assert_eq!(f.scan(CATEGORY_NOVEL, "backfill_done").await, Some(1));
    assert_eq!(f.scan(CATEGORY_SHORT, "backfill_done").await, Some(1));

    let forms = forms_by_isbn(&f.pool, &[isbn(1), isbn(4), isbn(99)]).await.unwrap();
    assert_eq!(forms.get(&isbn(1)), Some(&FORM_NOVEL));
    assert!(!forms.contains_key(&isbn(99)));
}

#[tokio::test]
async fn backfill_stops_once_books_older_than_2000_appear() {
    let f = fixture().await;
    f.page(CATEGORY_SHORT, 1, &[(10, "창비", "2000년 03월"), (11, "창비", "1999년 12월")]);
    f.page(CATEGORY_SHORT, 2, &[(12, "창비", "1998년 01월")]);
    f.fake.lock().unwrap().pages.insert((CATEGORY_NOVEL.to_string(), 1), vec![]);
    f.drain(1_000, 50).await;
    assert!(!f.requests().contains(&format!("list {CATEGORY_SHORT} 2")));
    assert_eq!(f.book(12).await, None);
    assert_eq!(f.book(10).await.unwrap().0, FORM_SHORT);
}

#[tokio::test]
async fn after_backfill_the_front_is_rechecked_until_only_known_books_show_up() {
    let f = fixture().await;
    f.page(CATEGORY_NOVEL, 1, &[(1, "창비", "2026년 10월")]);
    f.goods(1, Goods::Isbn(isbn(1)));
    f.drain(1_000, 50).await;
    f.requests();

    // 아직 때가 아니면 아무것도 안 한다
    assert_eq!(f.step(1_000 + SCAN_EVERY_SECS - 1).await, Step::Idle);

    // 새 책이 앞에 붙었다: 1쪽(새 책 + 아는 책)에서 끝나지 않고, 아는 책만 있는 2쪽에서 멈춘다
    f.page(CATEGORY_NOVEL, 1, &[(5, "창비", "2026년 11월"), (6, "민음사", "2026년 11월")]);
    f.page(CATEGORY_NOVEL, 2, &[(1, "창비", "2026년 10월")]);
    f.goods(5, Goods::Isbn(isbn(5)));
    f.goods(6, Goods::Isbn(isbn(6)));
    let later = 1_000 + SCAN_EVERY_SECS;
    f.drain(later, 50).await;
    let requests = f.requests();
    assert!(requests.contains(&format!("list {CATEGORY_NOVEL} 2")));
    assert!(!requests.contains(&format!("list {CATEGORY_NOVEL} 3")));
    assert_eq!(f.book(6).await.unwrap().1, Some(isbn(6)));
    assert_eq!(f.scan(CATEGORY_NOVEL, "scan_page").await, None);
    assert_eq!(f.scan(CATEGORY_NOVEL, "last_scan_at").await, Some(later));
}

#[tokio::test]
async fn blocked_list_stops_the_whole_worker_and_changes_nothing() {
    let f = fixture().await;
    f.fake.lock().unwrap().list_status = Some(429);
    assert_eq!(f.step(1_000).await, Step::Blocked);
    assert_eq!(f.scan(CATEGORY_NOVEL, "backfill_page").await, Some(1));
    assert_eq!(f.scan(CATEGORY_NOVEL, "retry_at").await, Some(0));
}

#[tokio::test]
async fn failing_list_page_is_retried_later_from_the_same_page() {
    let f = fixture().await;
    f.fake.lock().unwrap().list_status = Some(500);
    assert_eq!(f.step(1_000).await, Step::Processed);
    assert_eq!(f.scan(CATEGORY_NOVEL, "backfill_page").await, Some(1));
    assert_eq!(f.scan(CATEGORY_NOVEL, "retry_at").await, Some(1_000 + LIST_RETRY_SECS));
}

#[tokio::test]
async fn goods_without_isbn_or_failing_are_retried_with_delays() {
    let f = fixture().await;
    f.page(CATEGORY_NOVEL, 1, &[(1, "창비", "2026년 10월"), (2, "창비", "2026년 10월")]);
    f.goods(1, Goods::NoIsbn);
    f.goods(2, Goods::Status(503));
    assert_eq!(f.step(1_000).await, Step::Processed); // 1쪽
    assert_eq!(f.step(1_000).await, Step::Processed); // 상품 1
    assert_eq!(f.step(1_000).await, Step::Processed); // 상품 2
    assert_eq!(f.book(1).await.unwrap().2, STATUS_MISSING);
    assert_eq!(f.book(2).await.unwrap().2, STATUS_ERROR);
    let retry: i64 = query_scalar("SELECT next_try_at FROM yes24_books WHERE goods_no = 2")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(retry, 1_000 + RETRY_BASE_SECS);
}

#[tokio::test]
async fn blocked_goods_page_stops_the_worker_and_leaves_the_book_pending() {
    let f = fixture().await;
    f.page(CATEGORY_NOVEL, 1, &[(1, "창비", "2026년 10월")]);
    f.goods(1, Goods::Status(403));
    f.step(1_000).await;
    assert_eq!(f.step(1_000).await, Step::Blocked);
    assert_eq!(f.book(1).await.unwrap().2, STATUS_PENDING);
}

#[tokio::test]
async fn a_book_in_both_categories_counts_as_a_novel() {
    let f = fixture().await;
    for (no, form) in [(1, FORM_SHORT), (2, FORM_NOVEL)] {
        query(
            "INSERT INTO yes24_books (goods_no, form, title, isbn13, status, created_at) VALUES (?, ?, 't', ?, 'ok', 1)",
        )
        .bind(no)
        .bind(form)
        .bind(isbn(7))
        .execute(&f.pool)
        .await
        .unwrap();
    }
    assert_eq!(forms_by_isbn(&f.pool, &[isbn(7)]).await.unwrap().get(&isbn(7)), Some(&FORM_NOVEL));
}
