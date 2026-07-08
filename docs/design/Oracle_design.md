# Oracle T2 — Owner-check / IDOR–BOLA (GraphQL Query)

> Tài liệu thiết kế oracle cho nhánh **T2**: thiếu owner-check dẫn tới đọc trộm object private của victim.
> Phạm vi hiện tại: **Query** (read-only). Mutation sẽ mở rộng sau.

---

## 1. Phạm vi

T2 là bug ở **mức object**: cả một object lẽ ra không đọc được (vì thiếu owner-check), không phải chuyện một field lẻ bị phơi. Oracle chủ đạo là **differential hai account theo nguyên lý canary-containment**.

**Vì sao T2 là oracle sạch để làm chắc:**
- **Ground-truth rẻ nhất:** ta tự seed nên biết tuyệt đối object nào thuộc principal nào — không phải suy đoán chính sách.
- **Tín hiệu nhị phân, chính xác gần tuyệt đối** khi dùng canary (xem §3).
- **Read-only:** không cần chu trình setup → attack → verify; oracle chỉ là kiểm tra một response.

---

## 2. Định nghĩa bug

> **Thiếu owner-check trên một resolver path → principal A (attacker) đọc được object private thuộc principal B (victim).**

**Cái được đo là owner-check.** ID của object victim được feed thẳng từ seed vào attacker, **không** trộn với việc "ID có đoán được/lộ ra không" (ID-guessability là amplifier, là một nhánh test riêng nếu cần). Tách bạch để oracle T2 đo *đúng một thứ*: có owner-check hay không.

---

## 3. Nguyên lý oracle: canary-containment (KHÔNG so bằng nhau)

### Vì sao "kết quả A == kết quả B → bug" hỏng

- **Quá lỏng → false positive.** Object private của B rỗng / chỉ chứa giá trị mặc định → khi A bị chặn đúng, A nhận `null`/`{}`/rỗng; B cũng resolve ra rỗng → hai cái rỗng *bằng nhau* → đánh bug nhầm. Field volatile (timestamp, computed value) còn làm full-equality flaky.
- **Quá chặt → false negative.** A lấy được dữ liệu của B nhưng resolver A đính thêm/bớt field, hoặc route project khác đi → A ≠ B → **bỏ sót** leak có thật. Leak từng phần vẫn là bug nhưng so-bằng-nhau sẽ trượt.

### Luật đúng

> **Response của A (qua route R) _chứa_ canary của B  →  bug ở route R.**

Canary = giá trị duy nhất, không đoán được (UUID/token ngẫu nhiên), chỉ tồn tại trong object của B. Một UUID ngẫu nhiên không thể xuất hiện trong response hợp lệ của A một cách tình cờ → *có canary = leak*, đơn trị.

Ưu điểm:
- Miễn nhiễm bẫy rỗng (canary luôn khác rỗng).
- Bắt được leak từng phần (bất kỳ field canary nào lọt sang A đều tính).
- Không quan tâm A/B lệch vài field phụ.

Vai trò của "B resolve" đổi từ *đối tượng để so* sang **positive control** (chứng minh canary thật sự fetch được bởi chủ sở hữu).

---

## 4. Thiết kế seed

**Invariant bắt buộc:** A và B là **hai principal ngang quyền, không có quan hệ hợp lệ nào** (không cùng team/household/org, A không vô tình là admin). Nếu A và B có quan hệ chia sẻ hợp lệ thì A thấy đồ B là *đúng*, không phải bug.

Cho mỗi object type cần test, seed cho victim B **hai** object:
1. **Target** — `public: false`, chứa canary. Đây là object A *không được* chạm.
2. **Negative control** — `public: true`. A *nên* lấy được; nếu oracle flag cái này → đang over-flag intended sharing.

**Đặt canary đủ sâu.** Với watchlist, phần nhạy cảm nằm ở `items` (mã đang theo dõi) hoặc field lồng trên item (vd `items.notes`), không phải node `Watchlist` ở gốc. Owner-check có thể có ở `Watchlist` nhưng vắng ở resolver `watchlist.items`. Gieo canary ở **leaf nhạy cảm nhất**, và chọn field **trả raw** (field bị mask/transform sẽ làm canary không khớp — xem §8).

---

## 5. Controls

Để mọi kết luận đáng tin (đặc biệt: để "không match" nghĩa là *secure* chứ không phải *test chạy hụt*):

| Control | Thực hiện | Kỳ vọng | Ý nghĩa nếu sai |
|---------|-----------|---------|-----------------|
| **Positive — owner** | B resolve target qua route R | Trả ra canary | Canary không fetch được → test hỏng, không kết luận được |
| **Positive — attacker liveness** | A resolve object **của chính A** qua route R | Thành công | Route lỗi / token A hết hạn → "A không lấy được đồ B" vô nghĩa |
| **Negative — intended sharing** | A resolve object `public:true` của B | Lấy được, **không** bị flag | Nếu bị flag → oracle over-flag chia sẻ hợp lệ |

Chỉ khi cả hai positive control xanh thì "A không chạm được canary của B trên route R" mới là bằng chứng **secure** cho route đó.

---

## 6. Differential per-route

Engine `type → tất cả route tới type đó` là điểm mạnh kiến trúc — khai thác triệt để:

- Trong GraphQL, cùng một object victim thường tới được qua **nhiều resolver path**: trực tiếp `watchlist(id:)`, lồng `user(id: victim){ watchlists }`, hoặc lồng sâu qua type khác. Owner-check có thể có ở path trực tiếp nhưng **vắng** ở path lồng.
- Vì vậy T2 **không** phải "đổi ID rồi gọi lại một lần", mà là chạy differential trên **mọi path** engine liệt kê tới object type.
- So **per-route**: đối chiếu `A-qua-route-R` với **canary**, không đối chiếu `A-qua-route-1` với `B-qua-route-2`.
- **Ghi nhận bug theo từng route.** Route 1 an toàn, route 3 leak → là hai kết luận độc lập.

---

## 7. Đọc kết quả (đặc thù GraphQL)

A hitting object của B có (ít nhất) ba lớp response:

| Lớp | Biểu hiện | Kết luận |
|-----|-----------|----------|
| **Data leak** | Response A chứa canary của B | **BUG** |
| **Hard deny** | 403 / error / authz failure rõ ràng | Secure |
| **Soft deny / silent empty** | 200 + `data: { ...: null }` / rỗng | Secure (xác nhận bằng *vắng canary*, không bằng status) |

**Đừng tin status code.** GraphQL hay trả `data: { watchlist: null }` **kèm** mảng `errors` trong cùng một `200 OK`. Soi thẳng payload `data` để tìm canary — đó mới là tín hiệu duy nhất đáng tin.

---

## 8. Luật quyết định (pseudocode)

```text
for each object_type in scope:
    seed_target  = create_private_object(owner=B, canary=UUID, depth=deepest_sensitive_leaf)
    seed_control = create_public_object(owner=B)          # negative control
    a_own_object = create_private_object(owner=A)          # liveness control

    for each route R in paths_to(object_type):
        # --- controls ---
        assert canary in resolve(R, as=B, target=seed_target.id)      # owner positive
        assert resolve(R, as=A, target=a_own_object.id) is OK         # attacker liveness
        assert NOT flag(resolve(R, as=A, target=seed_control.id))     # intended-sharing negative

        # --- oracle ---
        resp = resolve(R, as=A, target=seed_target.id)
        if canary in payload_data(resp):     # inspect data tree, NOT status code
            report_bug(route=R, object_type=object_type)
```

---

## 9. Giả định & giới hạn

- **Đo owner-check**; ID feed từ seed, tách khỏi ID-guessability (nhánh riêng nếu cần).
- Canary phải nằm ở **field trả raw**. Field bị resolver biến đổi (vd `email` bị mask `j***@x.com`) sẽ làm canary-containment trượt → leak ở field transform có thể bị bỏ sót.
- Phạm vi Query/read-only; IDOR write/delete (mutation) cần chu trình setup → attack → verify riêng, không thuộc tài liệu này.

---

## 10. Câu hỏi mở

**Định nghĩa "chứa canary" như thế nào?**
- **(a) Exact field-value match** — biết trước canary nằm ở field nào, so đúng giá trị field đó. Chặt, ít false positive, nhưng cần map field chính xác.
- **(b) Quét toàn bộ response tree** — tìm canary ở bất kỳ đâu trong payload. Bắt được cả leak ở vị trí bất ngờ (field bị "rò" sang chỗ khác), nhưng cần chắc canary không trùng giá trị hợp lệ nào.

Lựa chọn (a) vs (b) quyết định oracle có bắt được leak ở field bị resolver biến đổi/di chuyển hay không. Hướng khả dĩ: quét tree (b) làm lưới rộng, rồi map về field (a) để classify + tính severity.