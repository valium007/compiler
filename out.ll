@formatString = private constant [6 x i8] c"%lld\0A\00"
declare i32 @printf(ptr, ...)

define i32 @main() {
bb_0:
  %v0_3 = add i64 0, 0
  %v1_2 = add i64 0, 0
  %v2_1 = add i64 0, 1
  br label %bb_1
bb_1:
  %v1_3 = phi i64 [ %v1_2, %bb_0 ], [ %v1_5, %bb_4 ]
  %v0_4 = phi i64 [ %v0_3, %bb_0 ], [ %v0_6, %bb_4 ]
  %v2_3 = phi i64 [ %v2_1, %bb_0 ], [ %v2_3, %bb_4 ]
  %v3_1 = add i64 0, 10
  %cmp_4_2_1 = icmp slt i64 %v1_3, %v3_1
  %v4_2 = zext i1 %cmp_4_2_1 to i64
  %br_cond_4_2_1 = icmp ne i64 %v4_2, 0
  br i1 %br_cond_4_2_1, label %bb_2, label %bb_5
bb_2:
  %v5_1 = add i64 0, 5
  %cmp_4_3_2 = icmp slt i64 %v1_3, %v5_1
  %v4_3 = zext i1 %cmp_4_3_2 to i64
  %br_cond_4_3_2 = icmp ne i64 %v4_3, 0
  br i1 %br_cond_4_3_2, label %bb_3, label %bb_4
bb_3:
  %v0_7 = add i64 %v0_4, %v2_3
  br label %bb_4
bb_4:
  %v0_5 = phi i64 [ %v0_4, %bb_2 ], [ %v0_7, %bb_3 ]
  %v6_1 = add i64 0, 2
  %v0_6 = mul i64 %v0_5, %v6_1
  %v1_5 = add i64 %v1_3, %v2_3
  br label %bb_1
bb_5:
  %fmt_ptr_0_4_5 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_0_4_5 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_0_4_5, i64 %v0_4)
  %v7_1 = add i64 0, 0
  %ret32_7_1_5 = trunc i64 %v7_1 to i32
  ret i32 %ret32_7_1_5
}
